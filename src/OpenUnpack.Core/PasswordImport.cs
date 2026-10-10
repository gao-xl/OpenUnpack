namespace OpenUnpack.Core;

public static class PasswordImport
{
    public const string TsvHeader = "#openunpack-tsv-v1";
    private const int MaxCharacters = 32 * 1024 * 1024;
    private const int MaxEntries = 100_000;

    public static PasswordImportResult ParseText(string text, string defaultGroup = "默认")
    {
        ArgumentNullException.ThrowIfNull(text);
        ArgumentNullException.ThrowIfNull(defaultGroup);
        if (text.Length > MaxCharacters)
            throw new InvalidDataException("导入文本过大，最多允许 32 Mi 个字符。");
        if (text.Length > 0 && text[0] == '\uFEFF') text = text[1..];
        // Only line separators and the initial BOM are removed. Password whitespace is meaningful.
        var lines = text.Replace("\r\n", "\n", StringComparison.Ordinal).Split('\n');
        var tsv = lines.Length > 0 && lines[0] == TsvHeader;
        var entries = new List<PasswordEntry>();
        var secrets = new HashSet<string>(StringComparer.Ordinal);
        var duplicates = 0;
        var ignored = 0;
        for (var lineIndex = tsv ? 1 : 0; lineIndex < lines.Length; lineIndex++)
        {
            var line = lines[lineIndex];
            if (line.Length == 0) { ignored++; continue; }
            PasswordEntry entry;
            if (tsv)
            {
                var columns = line.Split('\t');
                if (columns.Length != 4)
                    throw new FormatException($"TSV 第 {lineIndex + 1} 行必须包含 4 列：名称、密码、分组、备注。");
                if (columns[1].Length == 0) { ignored++; continue; }
                entry = new PasswordEntry
                {
                    Label = columns[0], Secret = columns[1],
                    Group = columns[2].Length == 0 ? defaultGroup : columns[2], Notes = columns[3]
                };
            }
            else
            {
                entry = new PasswordEntry { Label = $"导入密码 {entries.Count + 1}", Secret = line, Group = defaultGroup };
            }
            if (!secrets.Add(entry.Secret)) { duplicates++; continue; }
            entries.Add(entry);
            if (entries.Count > MaxEntries)
                throw new InvalidDataException("导入记录过多，最多允许 100000 条。");
        }
        return new PasswordImportResult(entries, duplicates, ignored);
    }

    public static int Merge(VaultSnapshot snapshot, IEnumerable<PasswordEntry> entries)
    {
        ArgumentNullException.ThrowIfNull(snapshot);
        ArgumentNullException.ThrowIfNull(entries);
        var secrets = new HashSet<string>(snapshot.Entries.Select(entry => entry.Secret), StringComparer.Ordinal);
        var ids = new HashSet<Guid>(snapshot.Entries.Select(entry => entry.Id));
        var additions = new List<PasswordEntry>();
        foreach (var entry in entries)
        {
            if (entry is null || string.IsNullOrEmpty(entry.Secret)) continue;
            if (!secrets.Add(entry.Secret)) continue;
            var next = entry;
            if (next.Id == Guid.Empty || !ids.Add(next.Id))
            {
                do { next = next with { Id = Guid.NewGuid() }; } while (!ids.Add(next.Id));
            }
            additions.Add(next);
            if (snapshot.Entries.Count + additions.Count > MaxEntries)
                throw new InvalidDataException("密码库记录过多，最多允许 100000 条。");
        }
        // Validate the whole import before modifying the destination.
        snapshot.Entries.AddRange(additions);
        return additions.Count;
    }
}
