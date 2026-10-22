using System.Text.RegularExpressions;

namespace OpenUnpack.Engine;

public sealed record ResolvedVolumeGroup(string FirstPath, string DisplayName, IReadOnlyList<string> Volumes);

public static partial class VolumeResolver
{
    public static ResolvedVolumeGroup ResolveSource(string path)
    {
        var full = Path.GetFullPath(path);
        if (!File.Exists(full))
        {
            return new(full, Path.GetFileName(full), [full]);
        }
        var directory = Path.GetDirectoryName(full) ?? "";
        var name = Path.GetFileName(full);

        var num = NumericVolume().Match(name);
        if (num.Success)
        {
            var parts = NumericParts(full);
            return new(parts[0], Path.GetFileName(num.Groups[1].Value), parts);
        }

        var rar = RarPart().Match(name);
        if (rar.Success)
        {
            var stem = rar.Groups[1].Value;
            var width = rar.Groups[2].Length;
            var volumes = FindNumbered(directory, stem + ".part", ".rar", out _);
            var first = Path.Combine(directory, stem + ".part" + 1.ToString().PadLeft(width, '0') + ".rar");
            return new(first, stem + ".rar", volumes);
        }

        var legacy = LegacyVolume().Match(name);
        var extension = Path.GetExtension(name);
        if (legacy.Success || extension.Equals(".zip", StringComparison.OrdinalIgnoreCase) || extension.Equals(".rar", StringComparison.OrdinalIgnoreCase))
        {
            var zip = legacy.Success ? legacy.Groups[2].Value.Equals("z", StringComparison.OrdinalIgnoreCase) : extension.Equals(".zip", StringComparison.OrdinalIgnoreCase);
            var stem = legacy.Success ? legacy.Groups[1].Value : Path.GetFileNameWithoutExtension(name);
            var first = Path.Combine(directory, stem + (zip ? ".zip" : ".rar"));
            var parts = FindNumbered(directory, stem + (zip ? ".z" : ".r"), "", out _);
            var volumes = new List<string>();
            if (!zip && File.Exists(first)) volumes.Add(first);
            volumes.AddRange(parts);
            if (zip && File.Exists(first)) volumes.Add(first);
            if (volumes.Count == 0 && File.Exists(full)) volumes.Add(full);
            return new(first, stem + (zip ? ".zip" : ".rar"), volumes);
        }

        return new(full, name, [full]);
    }

    public static IReadOnlyList<string> NumericParts(string firstPath)
    {
        var name = Path.GetFileName(firstPath);
        var match = NumericVolume().Match(name);
        if (!match.Success) return [firstPath];
        var directory = Path.GetDirectoryName(firstPath) ?? "";
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
            var match = pattern.Match(Path.GetFileName(path));
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
}
