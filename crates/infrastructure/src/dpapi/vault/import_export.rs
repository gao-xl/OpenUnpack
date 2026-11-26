//! 密码库 TXT/CSV/JSON 导入解析与解密候选密码变体生成。

use super::*;

pub(super) struct ParsedRecord {
    pub(super) line: usize,
    pub(super) name: String,
    pub(super) secret: String,
    pub(super) groups: Vec<String>,
    pub(super) notes: String,
    pub(super) error: bool,
    pub(super) message: String,
}

pub(super) fn parse_import_text(
    text: &str,
    format: &str,
    default_group: &str,
) -> Vec<ParsedRecord> {
    let lower_format = format.trim().to_ascii_lowercase();
    let is_tsv = lower_format == "tsv";
    let is_csv = lower_format == "csv" || lower_format.starts_with("csv:");
    let def_grp = default_group.trim();

    let mut results = if is_csv {
        parse_csv_import(text, &lower_format, def_grp)
    } else {
        let mut rows = Vec::new();
        for (idx, line) in text.lines().enumerate() {
            let line_num = idx + 1;
            let line_raw = line.trim_end_matches(['\r', '\n']);

            if is_tsv {
                let parts: Vec<&str> = line_raw.split('\t').collect();
                if parts.is_empty() || (parts.len() == 1 && parts[0].trim().is_empty()) {
                    continue;
                }

                if parts.len() != 4 {
                    rows.push(ParsedRecord {
                        line: line_num,
                        name: String::new(),
                        secret: String::new(),
                        groups: vec![],
                        notes: String::new(),
                        error: true,
                        message: "TSV 必须恰好为四列：名称、密码、分组、备注。".into(),
                    });
                    continue;
                }
                let (name, secret, grp_opt, notes) = (parts[0], parts[1], Some(parts[2]), parts[3]);

                if secret.is_empty() {
                    rows.push(ParsedRecord {
                        line: line_num,
                        name: name.trim().to_string(),
                        secret: String::new(),
                        groups: Vec::new(),
                        notes: notes.to_string(),
                        error: true,
                        message: "Empty secret".to_string(),
                    });
                    continue;
                }

                let mut grps = Vec::new();
                if let Some(g) = grp_opt {
                    for part in g.split([',', ';']) {
                        let t = part.trim();
                        if !t.is_empty() {
                            grps.push(t.to_string());
                        }
                    }
                }
                if grps.is_empty() && !def_grp.is_empty() {
                    grps.push(def_grp.to_string());
                }

                rows.push(ParsedRecord {
                    line: line_num,
                    name: name.trim().to_string(),
                    secret: secret.to_string(),
                    groups: grps,
                    notes: notes.to_string(),
                    error: false,
                    message: String::new(),
                });
            } else {
                // Plain text: first column/token is secret
                if line_raw.is_empty() {
                    continue;
                }
                let mut grps = Vec::new();
                if !def_grp.is_empty() {
                    grps.push(def_grp.to_string());
                }

                rows.push(ParsedRecord {
                    line: line_num,
                    name: format!("Line {}", line_num),
                    secret: line_raw.to_string(),
                    groups: grps,
                    notes: String::new(),
                    error: false,
                    message: String::new(),
                });
            }
        }
        rows
    };

    for record in &mut results {
        if !record.error
            && let Err(message) = validate_secret(&record.secret)
                .and_then(|_| validate_metadata(&record.name, &record.groups, &record.notes))
        {
            record.error = true;
            record.message = message;
            record.secret.zeroize();
            record.name.clear();
            record.groups.clear();
            record.notes.clear();
        }
    }
    results
}

#[derive(Default)]
pub(super) struct CsvColumnMapping {
    pub(super) name_idx: Option<usize>,
    pub(super) secret_idx: Option<usize>,
    pub(super) group_idx: Option<usize>,
    pub(super) notes_idx: Option<usize>,
}

pub(super) struct CsvRow {
    pub(super) line: usize,
    pub(super) columns: Vec<String>,
    pub(super) invalid_quotes: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum CsvFieldState {
    Start,
    Unquoted,
    Quoted,
    ClosedQuote,
    Invalid,
}

pub(super) fn parse_csv_rows(text: &str) -> Vec<CsvRow> {
    let mut rows = Vec::new();
    let mut current_row = Vec::new();
    let mut current_field = String::new();
    let mut state = CsvFieldState::Start;
    let mut invalid_quotes = false;
    let mut row_start_line = 1;
    let mut current_line = 1;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if state == CsvFieldState::Quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    current_field.push('"');
                } else {
                    state = CsvFieldState::ClosedQuote;
                }
            } else {
                if ch == '\r' {
                    current_field.push(ch);
                    if chars.peek() == Some(&'\n') {
                        current_field.push(chars.next().unwrap());
                    }
                    current_line += 1;
                    continue;
                } else if ch == '\n' {
                    current_line += 1;
                }
                current_field.push(ch);
            }
        } else {
            match ch {
                '"' => {
                    if state == CsvFieldState::Start {
                        state = CsvFieldState::Quoted;
                    } else {
                        invalid_quotes = true;
                        state = CsvFieldState::Invalid;
                    }
                }
                ',' => {
                    current_row.push(std::mem::take(&mut current_field));
                    state = CsvFieldState::Start;
                }
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    current_row.push(std::mem::take(&mut current_field));
                    if invalid_quotes
                        || !(current_row.is_empty()
                            || current_row.len() == 1 && current_row[0].is_empty())
                    {
                        rows.push(CsvRow {
                            line: row_start_line,
                            columns: std::mem::take(&mut current_row),
                            invalid_quotes,
                        });
                    } else {
                        current_row.clear();
                    }
                    current_line += 1;
                    row_start_line = current_line;
                    state = CsvFieldState::Start;
                    invalid_quotes = false;
                }
                '\n' => {
                    current_row.push(std::mem::take(&mut current_field));
                    if invalid_quotes
                        || !(current_row.is_empty()
                            || current_row.len() == 1 && current_row[0].is_empty())
                    {
                        rows.push(CsvRow {
                            line: row_start_line,
                            columns: std::mem::take(&mut current_row),
                            invalid_quotes,
                        });
                    } else {
                        current_row.clear();
                    }
                    current_line += 1;
                    row_start_line = current_line;
                    state = CsvFieldState::Start;
                    invalid_quotes = false;
                }
                _ => {
                    if state == CsvFieldState::ClosedQuote {
                        invalid_quotes = true;
                        state = CsvFieldState::Invalid;
                    } else if state == CsvFieldState::Start {
                        state = CsvFieldState::Unquoted;
                    }
                    current_field.push(ch);
                }
            }
        }
    }

    invalid_quotes |= state == CsvFieldState::Quoted;
    if invalid_quotes
        || state == CsvFieldState::ClosedQuote
        || !current_field.is_empty()
        || !current_row.is_empty()
    {
        current_row.push(std::mem::take(&mut current_field));
        rows.push(CsvRow {
            line: row_start_line,
            columns: current_row,
            invalid_quotes,
        });
    }

    rows
}

pub(super) fn parse_csv_import(text: &str, format: &str, def_grp: &str) -> Vec<ParsedRecord> {
    let rows = parse_csv_rows(text);
    if rows.is_empty() {
        return Vec::new();
    }

    let mut mapping = CsvColumnMapping::default();
    let mut data_start_idx = 0;

    // 检查显式列映射参数：csv:secret=1,name=0 或 csv:name,secret,group,notes
    if let Some(spec) = format.strip_prefix("csv:") {
        let spec_trimmed = spec.trim();
        if spec_trimmed.contains('=') {
            for kv in spec_trimmed.split(['&', ',']) {
                let mut parts = kv.split('=');
                if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                    let k = k.trim().to_ascii_lowercase();
                    let v_idx = v.trim().parse::<usize>().ok();
                    match k.as_str() {
                        "secret" | "password" | "pwd" => mapping.secret_idx = v_idx,
                        "name" | "title" => mapping.name_idx = v_idx,
                        "group" | "groups" => mapping.group_idx = v_idx,
                        "notes" | "note" => mapping.notes_idx = v_idx,
                        _ => {}
                    }
                }
            }
        } else if spec_trimmed.contains(',') {
            let cols: Vec<&str> = spec_trimmed.split(',').map(|s| s.trim()).collect();
            for (idx, col) in cols.iter().enumerate() {
                match col.to_ascii_lowercase().as_str() {
                    "secret" | "password" | "pwd" => mapping.secret_idx = Some(idx),
                    "name" | "title" => mapping.name_idx = Some(idx),
                    "group" | "groups" => mapping.group_idx = Some(idx),
                    "notes" | "note" => mapping.notes_idx = Some(idx),
                    _ => {}
                }
            }
        }
    }

    // 若无显式映射，探测第一行是否为表头
    if mapping.secret_idx.is_none() && !rows[0].invalid_quotes {
        let first_row = &rows[0].columns;
        let mut header_has_secret = false;
        let mut detected_map = CsvColumnMapping::default();

        for (idx, cell) in first_row.iter().enumerate() {
            let lower = cell.to_ascii_lowercase();
            if lower.contains("password")
                || lower.contains("secret")
                || lower.contains("pwd")
                || lower.contains("密码")
                || lower.contains("解压码")
                || lower.contains("提取码")
            {
                detected_map.secret_idx = Some(idx);
                header_has_secret = true;
            } else if lower.contains("name") || lower.contains("名称") || lower.contains("标题")
            {
                detected_map.name_idx = Some(idx);
            } else if lower.contains("group") || lower.contains("分组") || lower.contains("标签")
            {
                detected_map.group_idx = Some(idx);
            } else if lower.contains("notes")
                || lower.contains("note")
                || lower.contains("备注")
                || lower.contains("说明")
            {
                detected_map.notes_idx = Some(idx);
            }
        }

        if header_has_secret {
            mapping = detected_map;
            data_start_idx = 1;
        }
    }

    let mut results = Vec::new();
    for row in rows.into_iter().skip(data_start_idx) {
        let line_num = row.line;
        let cols = row.columns;
        if row.invalid_quotes {
            results.push(ParsedRecord {
                line: line_num,
                name: String::new(),
                secret: String::new(),
                groups: Vec::new(),
                notes: String::new(),
                error: true,
                message: "CSV 引号格式无效：引号字段必须完整闭合，且字段内引号必须写为两个双引号。"
                    .into(),
            });
            continue;
        }
        if cols.is_empty() || (cols.len() == 1 && cols[0].is_empty()) {
            continue;
        }

        let (name, secret, grp_opt, notes) = if mapping.secret_idx.is_some() {
            let secret = mapping
                .secret_idx
                .and_then(|i| cols.get(i))
                .cloned()
                .unwrap_or_default();
            let name = mapping
                .name_idx
                .and_then(|i| cols.get(i))
                .cloned()
                .unwrap_or_default();
            let grp = mapping.group_idx.and_then(|i| cols.get(i)).cloned();
            let notes = mapping
                .notes_idx
                .and_then(|i| cols.get(i))
                .cloned()
                .unwrap_or_default();
            (name, secret, grp, notes)
        } else {
            // 根据列数自动对齐
            match cols.len() {
                1 => (String::new(), cols[0].clone(), None, String::new()),
                2 => (cols[0].clone(), cols[1].clone(), None, String::new()),
                3 => (
                    cols[0].clone(),
                    cols[1].clone(),
                    Some(cols[2].clone()),
                    String::new(),
                ),
                _ => (
                    cols[0].clone(),
                    cols[1].clone(),
                    Some(cols[2].clone()),
                    cols[3..].join(" "),
                ),
            }
        };

        if secret.is_empty() {
            results.push(ParsedRecord {
                line: line_num,
                name: name.trim().to_string(),
                secret: String::new(),
                groups: Vec::new(),
                notes: notes.to_string(),
                error: true,
                message: "Empty secret".to_string(),
            });
            continue;
        }

        let mut grps = Vec::new();
        if let Some(g) = grp_opt {
            for part in g.split([',', ';']) {
                let t = part.trim();
                if !t.is_empty() {
                    grps.push(t.to_string());
                }
            }
        }
        if grps.is_empty() && !def_grp.is_empty() {
            grps.push(def_grp.to_string());
        }

        let display_name = if name.trim().is_empty() {
            format!("Row {}", line_num)
        } else {
            name.trim().to_string()
        };

        results.push(ParsedRecord {
            line: line_num,
            name: display_name,
            secret,
            groups: grps,
            notes,
            error: false,
            message: String::new(),
        });
    }

    results
}

#[derive(PartialEq, Eq, Hash)]
pub(super) struct TemporarySecret(String);
impl Drop for TemporarySecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// 基于常见规则为已保存候选词派生受限变体（大小写、去修饰符、分隔符替换等）
pub fn derive_candidate_mutations(base_candidates: &[Candidate]) -> Vec<Candidate> {
    let mut existing: std::collections::HashSet<TemporarySecret> = base_candidates
        .iter()
        .map(|c| TemporarySecret(c.secret.clone()))
        .collect();
    let mut mutations = Vec::new();

    for base in base_candidates {
        let trimmed = base.secret.trim();
        if trimmed.is_empty() || trimmed.len() > 256 {
            continue;
        }

        let mut variants = Vec::new();

        // 1. 剥离括号与修饰符
        if let Some(inner) = strip_affixes_and_tags(trimmed)
            && !inner.is_empty()
            && inner != trimmed
        {
            variants.push(Zeroizing::new(inner));
        }

        // 2. 大小写变换
        if trimmed.chars().any(|c| c.is_ascii_uppercase()) {
            variants.push(Zeroizing::new(trimmed.to_ascii_lowercase()));
        }
        if trimmed.chars().any(|c| c.is_ascii_lowercase()) {
            variants.push(Zeroizing::new(trimmed.to_ascii_uppercase()));
        }

        // 3. 分隔符互换（_ 与 -）
        if trimmed.contains('_') && !trimmed.contains('-') {
            variants.push(Zeroizing::new(trimmed.replace('_', "-")));
        } else if trimmed.contains('-') && !trimmed.contains('_') {
            variants.push(Zeroizing::new(trimmed.replace('-', "_")));
        }

        // 4. 剥离内部空格（防止复制带入的杂质空白）
        if trimmed.contains(' ') {
            let no_space = trimmed.replace(' ', "");
            if !no_space.is_empty() {
                variants.push(Zeroizing::new(no_space));
            }
        }

        // 5. 域名互补派生（www. 补充）
        let lower = trimmed.to_ascii_lowercase();
        if !lower.starts_with("www.")
            && !lower.starts_with("http")
            && (lower.ends_with(".com")
                || lower.ends_with(".cn")
                || lower.ends_with(".net")
                || lower.ends_with(".org")
                || lower.ends_with(".cc")
                || lower.ends_with(".me")
                || lower.ends_with(".top"))
        {
            variants.push(Zeroizing::new(format!("www.{trimmed}")));
        }

        let mut count = 0;
        for mut var in variants {
            if count >= 3 {
                break;
            }
            if !var.is_empty()
                && var.len() <= 4096
                && existing.insert(TemporarySecret(var.to_string()))
            {
                count += 1;
                mutations.push(Candidate {
                    id: format!("{}#m{}", base.id, count),
                    revision: base.revision,
                    label: format!("{} (变体)", base.label),
                    secret: std::mem::take(&mut *var),
                });
            }
        }
    }

    mutations
}

pub(super) fn strip_affixes_and_tags(secret: &str) -> Option<String> {
    let mut s = secret.trim();
    let mut changed = false;

    if ((s.starts_with('[') && s.ends_with(']'))
        || (s.starts_with('【') && s.ends_with('】'))
        || (s.starts_with('(') && s.ends_with(')'))
        || (s.starts_with('（') && s.ends_with('）')))
        && let (Some(first), Some(last)) = (s.chars().next(), s.chars().next_back())
    {
        s = s[first.len_utf8()..s.len() - last.len_utf8()].trim();
        changed = true;
    }

    let prefixes = [
        "密码:",
        "密码：",
        "密码=",
        "解压密码:",
        "解压密码：",
        "解压密码=",
        "pwd:",
        "pwd：",
        "pwd=",
        "pass:",
        "pass：",
        "pass=",
        "code:",
        "code：",
        "code=",
        "解压码:",
        "解压码：",
        "解压码=",
        "提取码:",
        "提取码：",
        "提取码=",
        "提取:",
        "提取：",
        "提取=",
    ];
    for p in prefixes {
        if let Some(stripped) = s.strip_prefix(p) {
            s = stripped.trim();
            changed = true;
            break;
        }
        if s.get(..p.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(p))
        {
            s = s[p.len()..].trim();
            changed = true;
            break;
        }
    }

    if let Some(rest) = s.strip_prefix("https://") {
        s = rest.trim_start_matches("www.").trim();
        changed = true;
    } else if let Some(rest) = s.strip_prefix("http://") {
        s = rest.trim_start_matches("www.").trim();
        changed = true;
    } else if let Some(rest) = s.strip_prefix("www.") {
        s = rest.trim();
        changed = true;
    }

    let trimmed_punct = s
        .trim_end_matches(|c: char| {
            c == '.'
                || c == ','
                || c == '!'
                || c == '?'
                || c == ';'
                || c == ':'
                || c == '。'
                || c == '！'
                || c == '；'
                || c == '：'
        })
        .trim();
    if trimmed_punct != s {
        s = trimmed_punct;
        changed = true;
    }

    if changed && !s.is_empty() {
        Some(s.to_string())
    } else {
        None
    }
}
