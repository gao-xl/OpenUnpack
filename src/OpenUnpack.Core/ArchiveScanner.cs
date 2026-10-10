using System.Text.RegularExpressions;

namespace OpenUnpack.Core;

public static partial class ArchiveScanner
{
    public static ScanResult Scan(IEnumerable<string> paths, bool recursive = true)
    {
        var files = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var notices = new List<ScanNotice>();
        foreach (var input in paths)
        {
            try
            {
                var path = System.IO.Path.GetFullPath(input);
                if (File.Exists(path)) files.Add(path);
                else if (Directory.Exists(path)) CollectDirectory(path, recursive, files, notices);
                else notices.Add(new(input, "文件或文件夹不存在。"));
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
            { notices.Add(new(input, "无法读取此路径。")); }
        }

        var archives = new Dictionary<string, ArchiveSource>(StringComparer.OrdinalIgnoreCase);
        foreach (var path in files.Order(StringComparer.OrdinalIgnoreCase))
        {
            if (!IsSupported(path)) continue;
            try
            {
                var source = ResolveSource(path);
                archives.TryAdd(source.Path, source);
                if (!File.Exists(source.Path)) notices.Add(new(path, "分卷缺少起始卷，解压时将报告缺卷。"));
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            { notices.Add(new(path, "无法读取压缩包所在文件夹。")); }
        }
        return new(archives.Values.ToArray(), notices);
    }

    private static void CollectDirectory(string directory, bool recursive, HashSet<string> files, List<ScanNotice> notices)
    {
        try
        {
            if ((File.GetAttributes(directory) & FileAttributes.ReparsePoint) != 0)
            { notices.Add(new(directory, "已跳过链接文件夹。")); return; }
            foreach (var path in Directory.EnumerateFiles(directory))
            {
                if ((File.GetAttributes(path) & FileAttributes.ReparsePoint) == 0) files.Add(path);
                else notices.Add(new(path, "已跳过链接文件。"));
            }
            if (recursive)
                foreach (var child in Directory.EnumerateDirectories(directory)) CollectDirectory(child, true, files, notices);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        { notices.Add(new(directory, "文件夹读取失败，已继续扫描其他路径。")); }
    }

    internal static bool IsSupported(string path)
    {
        var name = System.IO.Path.GetFileName(path);
        return NumericVolume().IsMatch(name) || RarPart().IsMatch(name) || LegacyVolume().IsMatch(name)
            || System.IO.Path.GetExtension(name).ToLowerInvariant() is ".zip" or ".7z" or ".rar" or ".tar" or ".gz" or ".bz2" or ".xz" or ".cab";
    }

    internal static ArchiveSource ResolveSource(string input)
    {
        var path = System.IO.Path.GetFullPath(input);
        var directory = System.IO.Path.GetDirectoryName(path)!;
        var name = System.IO.Path.GetFileName(path);
        var numeric = NumericVolume().Match(name);
        if (numeric.Success)
        {
            var stem = numeric.Groups[1].Value;
            var width = numeric.Groups[2].Length;
            var volumes = FindNumbered(directory, stem + ".", "", out _);
            var first = System.IO.Path.Combine(directory, stem + "." + 1.ToString().PadLeft(width, '0'));
            return new(first, stem, volumes);
        }
        var rar = RarPart().Match(name);
        if (rar.Success)
        {
            var stem = rar.Groups[1].Value;
            var width = rar.Groups[2].Length;
            var volumes = FindNumbered(directory, stem + ".part", ".rar", out _);
            var first = System.IO.Path.Combine(directory, stem + ".part" + 1.ToString().PadLeft(width, '0') + ".rar");
            return new(first, stem + ".rar", volumes);
        }
        var legacy = LegacyVolume().Match(name);
        var extension = System.IO.Path.GetExtension(name);
        if (legacy.Success || extension.Equals(".zip", StringComparison.OrdinalIgnoreCase) || extension.Equals(".rar", StringComparison.OrdinalIgnoreCase))
        {
            var zip = legacy.Success ? legacy.Groups[2].Value.Equals("z", StringComparison.OrdinalIgnoreCase) : extension.Equals(".zip", StringComparison.OrdinalIgnoreCase);
            var stem = legacy.Success ? legacy.Groups[1].Value : System.IO.Path.GetFileNameWithoutExtension(name);
            var first = System.IO.Path.Combine(directory, stem + (zip ? ".zip" : ".rar"));
            var parts = FindNumbered(directory, stem + (zip ? ".z" : ".r"), "", out _);
            var volumes = new List<string>();
            if (!zip && File.Exists(first)) volumes.Add(first);
            volumes.AddRange(parts);
            if (zip && File.Exists(first)) volumes.Add(first);
            if (volumes.Count == 0 && File.Exists(path)) volumes.Add(path);
            return new(first, stem + (zip ? ".zip" : ".rar"), volumes);
        }
        return new(path, name, [path]);
    }

    internal static IReadOnlyList<string> NumericParts(string firstPath)
    {
        var name = System.IO.Path.GetFileName(firstPath);
        var match = NumericVolume().Match(name);
        if (!match.Success) return [firstPath];
        var directory = System.IO.Path.GetDirectoryName(firstPath)!;
        var parts = FindNumbered(directory, match.Groups[1].Value + ".", "", out var numbers);
        if (parts.Count == 0 || numbers[0] != 1)
            throw new ArchiveException(ArchiveFailure.MissingVolume, "分卷缺少起始卷。请将所有分卷放在同一文件夹。");
        for (var i = 0; i < numbers.Count; i++)
            if (numbers[i] != i + 1) throw new ArchiveException(ArchiveFailure.MissingVolume, "分卷编号不连续，请检查缺少的分卷。");
        return parts;
    }

    private static IReadOnlyList<string> FindNumbered(string directory, string prefix, string suffix, out List<int> numbers)
    {
        var pattern = new Regex("^" + Regex.Escape(prefix) + @"(\d+)" + Regex.Escape(suffix) + "$", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
        var values = new List<(int Number, string Path)>();
        foreach (var path in Directory.EnumerateFiles(directory))
        {
            var match = pattern.Match(System.IO.Path.GetFileName(path));
            if (match.Success && int.TryParse(match.Groups[1].Value, out var number)) values.Add((number, path));
        }
        values.Sort((a, b) => a.Number.CompareTo(b.Number));
        numbers = values.Select(v => v.Number).ToList();
        return values.Select(v => v.Path).ToArray();
    }

    [GeneratedRegex(@"^(.+\.(?:7z|zip))\.(\d{3,})$", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant)]
    private static partial Regex NumericVolume();
    [GeneratedRegex(@"^(.+)\.part(\d+)\.rar$", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant)]
    private static partial Regex RarPart();
    [GeneratedRegex(@"^(.+)\.([rz])(\d{2,})$", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant)]
    private static partial Regex LegacyVolume();
    [GeneratedRegex(@"(?i)(?:密码|pwd|password|解压码|提取码)[：:\s_-]*([^\s\[\]【】()（）]+)")]
    private static partial Regex ExplicitPasswordRegex();
    [GeneratedRegex(@"[\[【(]([^\[\]【】()（）]+)[\]】)]")]
    private static partial Regex BracketHintRegex();

    public static string? ExtractPasswordFromFilename(string? filename)
    {
        if (string.IsNullOrWhiteSpace(filename)) return null;
        var name = System.IO.Path.GetFileName(filename);
        name = NumericVolume().Replace(name, "$1");
        name = RarPart().Replace(name, "$1");
        name = LegacyVolume().Replace(name, "$1");
        var ext = System.IO.Path.GetExtension(name);
        if (ext.Equals(".zip", StringComparison.OrdinalIgnoreCase)
            || ext.Equals(".7z", StringComparison.OrdinalIgnoreCase)
            || ext.Equals(".rar", StringComparison.OrdinalIgnoreCase)
            || ext.Equals(".tar", StringComparison.OrdinalIgnoreCase)
            || ext.Equals(".gz", StringComparison.OrdinalIgnoreCase)
            || ext.Equals(".bz2", StringComparison.OrdinalIgnoreCase)
            || ext.Equals(".xz", StringComparison.OrdinalIgnoreCase)
            || ext.Equals(".cab", StringComparison.OrdinalIgnoreCase))
        {
            name = System.IO.Path.GetFileNameWithoutExtension(name);
        }

        foreach (Match match in BracketHintRegex().Matches(name))
        {
            var inner = match.Groups[1].Value.Trim();
            var subMatch = ExplicitPasswordRegex().Match(inner);
            if (subMatch.Success)
            {
                var s = subMatch.Groups[1].Value.Trim();
                if (!string.IsNullOrEmpty(s) && s.Length <= 128) return s;
            }
        }

        var explicitMatch = ExplicitPasswordRegex().Match(name);
        if (explicitMatch.Success)
        {
            var s = explicitMatch.Groups[1].Value.Trim();
            if (!string.IsNullOrEmpty(s) && s.Length <= 128) return s;
        }

        return null;
    }
}
