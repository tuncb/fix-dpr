use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::pas_lex;
use crate::unit_cache::{self, UnitCache, UnitFileInfo};

#[derive(Debug)]
pub struct DprUpdateSummary {
    pub scanned: usize,
    pub updated: usize,
    pub updated_paths: Vec<PathBuf>,
    pub warnings: Vec<String>,
    pub failures: usize,
}

#[derive(Debug)]
struct UsesEntry {
    name: String,
    in_path: Option<String>,
    start: usize,
    delimiter: Option<u8>,
}

#[derive(Debug)]
struct UsesList {
    entries: Vec<UsesEntry>,
    semicolon: usize,
    multiline: bool,
    indent: String,
    has_backslash: bool,
    has_slash: bool,
}

pub fn update_dpr_files(
    dpr_paths: &[PathBuf],
    unit_cache: &mut UnitCache,
    new_unit: &UnitFileInfo,
) -> io::Result<DprUpdateSummary> {
    let mut summary = DprUpdateSummary {
        scanned: 0,
        updated: 0,
        updated_paths: Vec::new(),
        warnings: Vec::new(),
        failures: 0,
    };

    for path in dpr_paths {
        summary.scanned += 1;
        let bytes = match fs::read(path) {
            Ok(data) => data,
            Err(err) => {
                summary.warnings.push(format!(
                    "warning: failed to read dpr {}: {err}",
                    path.display()
                ));
                summary.failures += 1;
                continue;
            }
        };
        let Some(list) = parse_dpr_uses(&bytes) else {
            summary
                .warnings
                .push(format!("warning: no uses list found in {}", path.display()));
            summary.failures += 1;
            continue;
        };

        let project_map = build_project_map(path, &list, unit_cache, &mut summary.warnings);
        if list
            .entries
            .iter()
            .any(|entry| entry.name.eq_ignore_ascii_case(&new_unit.name))
        {
            continue;
        }
        if project_map.is_empty() {
            continue;
        }

        let dependents =
            compute_project_dependents(unit_cache, &project_map, new_unit, &mut summary.warnings)?;

        let mut needs_update = false;
        for entry in &list.entries {
            let key = entry.name.to_ascii_lowercase();
            if let Some(path) = project_map.get(&key) {
                if let Some(&id) = dependents.id_by_path.get(path) {
                    if dependents.dependents[id] {
                        needs_update = true;
                        break;
                    }
                }
            }
        }

        if !needs_update {
            continue;
        }

        let updated = match insert_new_unit(&bytes, path, &list, new_unit) {
            Ok(value) => value,
            Err(err) => {
                summary.warnings.push(format!(
                    "warning: failed to update dpr {}: {err}",
                    path.display()
                ));
                summary.failures += 1;
                continue;
            }
        };
        if updated {
            summary.updated += 1;
            summary.updated_paths.push(path.clone());
        }
    }

    Ok(summary)
}

struct ProjectDependents {
    dependents: Vec<bool>,
    id_by_path: HashMap<PathBuf, usize>,
}

fn build_project_map(
    dpr_path: &Path,
    list: &UsesList,
    unit_cache: &UnitCache,
    warnings: &mut Vec<String>,
) -> HashMap<String, PathBuf> {
    let mut map = HashMap::new();

    for entry in &list.entries {
        let Some(raw_path) = entry.in_path.as_ref() else {
            match resolve_by_name(unit_cache, &entry.name) {
                ResolveByName::NotFound => {}
                ResolveByName::Unique(fallback) => {
                    warnings.push(format!(
                        "warning: missing in-path for unit {} in {} (resolved via scan)",
                        entry.name,
                        dpr_path.display()
                    ));
                    insert_project_entry(&mut map, entry, fallback, dpr_path, warnings);
                }
                ResolveByName::Ambiguous(count) => {
                    warnings.push(format!(
                        "warning: missing in-path for unit {} in {} ({} matches)",
                        entry.name,
                        dpr_path.display(),
                        count
                    ));
                }
            }
            continue;
        };

        let resolved = resolve_dpr_unit_path(dpr_path, raw_path);
        if !resolved.is_file() {
            warnings.push(format!(
                "warning: dpr uses path not found for unit {} in {}: {}",
                entry.name,
                dpr_path.display(),
                resolved.display()
            ));
            match resolve_by_name(unit_cache, &entry.name) {
                ResolveByName::Unique(fallback) => {
                    insert_project_entry(&mut map, entry, fallback, dpr_path, warnings);
                }
                ResolveByName::Ambiguous(count) => {
                    warnings.push(format!(
                        "warning: unit {} referenced in {} is ambiguous ({} matches)",
                        entry.name,
                        dpr_path.display(),
                        count
                    ));
                }
                ResolveByName::NotFound => {}
            }
            continue;
        }

        insert_project_entry(&mut map, entry, resolved, dpr_path, warnings);
    }

    map
}

fn insert_project_entry(
    map: &mut HashMap<String, PathBuf>,
    entry: &UsesEntry,
    resolved: PathBuf,
    dpr_path: &Path,
    warnings: &mut Vec<String>,
) {
    let key = entry.name.to_ascii_lowercase();
    if let Some(existing) = map.get(&key) {
        if existing != &resolved {
            warnings.push(format!(
                "warning: duplicate unit name {} in {} with multiple paths",
                entry.name,
                dpr_path.display()
            ));
        }
        return;
    }
    map.insert(key, resolved);
}

enum ResolveByName {
    NotFound,
    Unique(PathBuf),
    Ambiguous(usize),
}

fn resolve_by_name(unit_cache: &UnitCache, unit_name: &str) -> ResolveByName {
    let key = unit_name.to_ascii_lowercase();
    let Some(paths) = unit_cache.by_name.get(&key) else {
        return ResolveByName::NotFound;
    };
    if paths.len() > 1 {
        return ResolveByName::Ambiguous(paths.len());
    }
    ResolveByName::Unique(paths[0].clone())
}

fn compute_project_dependents(
    unit_cache: &mut UnitCache,
    project_map: &HashMap<String, PathBuf>,
    new_unit: &UnitFileInfo,
    warnings: &mut Vec<String>,
) -> io::Result<ProjectDependents> {
    let mut id_by_path = HashMap::new();
    let mut rev: Vec<Vec<usize>> = Vec::new();
    let mut direct: Vec<bool> = Vec::new();
    let mut queue = VecDeque::new();

    for path in project_map.values() {
        if id_by_path.contains_key(path) {
            continue;
        }
        let id = id_by_path.len();
        id_by_path.insert(path.clone(), id);
        rev.push(Vec::new());
        direct.push(false);
        queue.push_back(path.clone());
    }

    while let Some(unit_path) = queue.pop_front() {
        let uses = {
            let Some(info) = unit_cache::get_or_load(unit_cache, &unit_path, warnings)? else {
                warnings.push(format!(
                    "warning: failed to read unit at {}",
                    unit_path.display()
                ));
                continue;
            };
            info.uses.clone()
        };
        let Some(&source_id) = id_by_path.get(&unit_path) else {
            continue;
        };

        for dep in uses {
            if dep.eq_ignore_ascii_case(&new_unit.name) {
                direct[source_id] = true;
                continue;
            }
            let dep_path = resolve_dep_path(
                project_map,
                unit_cache,
                dep.as_str(),
                unit_path.as_path(),
                warnings,
            );
            let Some(dep_path) = dep_path else {
                continue;
            };
            let target_id = if let Some(&id) = id_by_path.get(&dep_path) {
                id
            } else {
                let id = id_by_path.len();
                id_by_path.insert(dep_path.clone(), id);
                rev.push(Vec::new());
                direct.push(false);
                queue.push_back(dep_path.clone());
                id
            };
            rev[target_id].push(source_id);
        }
    }

    let mut dependents = vec![false; id_by_path.len()];
    let mut queue = VecDeque::new();
    for (id, is_direct) in direct.iter().copied().enumerate() {
        if is_direct {
            dependents[id] = true;
            queue.push_back(id);
        }
    }

    while let Some(current) = queue.pop_front() {
        for &next in &rev[current] {
            if !dependents[next] {
                dependents[next] = true;
                queue.push_back(next);
            }
        }
    }

    Ok(ProjectDependents {
        dependents,
        id_by_path,
    })
}

fn resolve_dep_path(
    project_map: &HashMap<String, PathBuf>,
    unit_cache: &UnitCache,
    dep_name: &str,
    source_path: &Path,
    warnings: &mut Vec<String>,
) -> Option<PathBuf> {
    let dep_key = dep_name.to_ascii_lowercase();
    if let Some(path) = project_map.get(&dep_key) {
        return Some(path.clone());
    }
    if let Some(paths) = unit_cache.by_name.get(&dep_key) {
        if paths.len() == 1 {
            return Some(paths[0].clone());
        }
        warnings.push(format!(
            "warning: ambiguous unit {} referenced by {} ({} matches)",
            dep_name,
            source_path.display(),
            paths.len()
        ));
        return None;
    }
    None
}

fn resolve_dpr_unit_path(dpr_path: &Path, raw: &str) -> PathBuf {
    let candidate = PathBuf::from(raw);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        dpr_path
            .parent()
            .map(|parent| parent.join(&candidate))
            .unwrap_or(candidate)
    };
    unit_cache::canonicalize_if_exists(&resolved)
}

fn insert_new_unit(
    bytes: &[u8],
    dpr_path: &Path,
    list: &UsesList,
    new_unit: &UnitFileInfo,
) -> io::Result<bool> {
    let rel_path = relative_path(&new_unit.path, dpr_path.parent());
    let separator = if list.has_backslash {
        '\\'
    } else if list.has_slash {
        '/'
    } else {
        '\\'
    };
    let separator_str = separator.to_string();
    let rel_path = rel_path.replace(['\\', '/'], &separator_str);
    let entry_text = format!("{} in '{}'", new_unit.name, rel_path);

    let line_ending = detect_line_ending(bytes);
    let last_delim = list.entries.last().and_then(|entry| entry.delimiter);
    let insertion = if list.multiline {
        let prefix = if matches!(last_delim, Some(b',')) {
            ""
        } else {
            ","
        };
        format!("{prefix}{line_ending}{}{}", list.indent, entry_text)
    } else {
        let prefix = if matches!(last_delim, Some(b',')) {
            " "
        } else {
            ", "
        };
        format!("{prefix}{entry_text}")
    };

    let insert_at = if list.multiline && !matches!(last_delim, Some(b',')) {
        let mut pos = list.semicolon;
        while pos > 0 && bytes[pos - 1].is_ascii_whitespace() {
            pos -= 1;
        }
        pos
    } else {
        list.semicolon
    };

    let insert_bytes = insertion.as_bytes();
    let mut output = Vec::with_capacity(bytes.len() + insert_bytes.len());
    output.extend_from_slice(&bytes[..insert_at]);
    output.extend_from_slice(insert_bytes);
    output.extend_from_slice(&bytes[insert_at..]);

    write_atomic(dpr_path, &output)?;
    Ok(true)
}

fn relative_path(target: &Path, base: Option<&Path>) -> String {
    if let Some(base) = base {
        if let Some(diff) = pathdiff::diff_paths(target, base) {
            return diff.to_string_lossy().to_string();
        }
    }
    target.to_string_lossy().to_string()
}

fn parse_dpr_uses(bytes: &[u8]) -> Option<UsesList> {
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => i = pas_lex::skip_brace_comment(bytes, i + 1),
            b'(' if bytes.get(i + 1) == Some(&b'*') => {
                i = pas_lex::skip_paren_comment(bytes, i + 2)
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = pas_lex::skip_line_comment(bytes, i + 2),
            b'\'' => i = pas_lex::skip_string(bytes, i + 1),
            byte if pas_lex::is_ident_start(byte) => {
                let (token, next) = pas_lex::read_ident(bytes, i);
                if token.eq_ignore_ascii_case("uses") {
                    return parse_dpr_uses_list(bytes, next);
                }
                i = next;
            }
            _ => i += 1,
        }
    }
    None
}

fn parse_dpr_uses_list(bytes: &[u8], mut i: usize) -> Option<UsesList> {
    let list_start = i;
    let mut entries = Vec::new();
    let mut semicolon = None;
    let mut has_backslash = false;
    let mut has_slash = false;

    loop {
        i = pas_lex::skip_ws_and_comments(bytes, i);
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b';' {
            semicolon = Some(i);
            break;
        }
        if !pas_lex::is_ident_start(bytes[i]) {
            i += 1;
            continue;
        }

        let entry_start = i;
        let (name, next) = pas_lex::read_ident_with_dots(bytes, i);
        i = next;
        i = pas_lex::skip_ws_and_comments(bytes, i);

        let (in_path, pos, delim) = scan_entry_tail(bytes, i);
        update_separator_flags(bytes, entry_start, pos, &mut has_backslash, &mut has_slash);
        entries.push(UsesEntry {
            name,
            in_path,
            start: entry_start,
            delimiter: delim,
        });
        match delim {
            Some(b',') => i = pos + 1,
            Some(b';') => {
                semicolon = Some(pos);
                break;
            }
            _ => {
                break;
            }
        }
    }

    let semicolon = semicolon?;
    if entries.is_empty() {
        return None;
    }
    let multiline = bytes[list_start..semicolon].contains(&b'\n');
    let indent = if multiline {
        entries
            .first()
            .map(|entry| infer_indent(bytes, entry.start))
            .unwrap_or_default()
    } else {
        String::new()
    };

    Some(UsesList {
        entries,
        semicolon,
        multiline,
        indent,
        has_backslash,
        has_slash,
    })
}

fn infer_indent(bytes: &[u8], entry_start: usize) -> String {
    let line_start = bytes[..entry_start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|pos| pos + 1)
        .unwrap_or(0);
    let indent_bytes = &bytes[line_start..entry_start];
    let indent = indent_bytes
        .iter()
        .take_while(|&&b| b == b' ' || b == b'\t')
        .copied()
        .collect::<Vec<_>>();
    String::from_utf8_lossy(&indent).to_string()
}

fn skip_ws_and_comments_no_strings(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'{' => i = pas_lex::skip_brace_comment(bytes, i + 1),
            b'(' if bytes.get(i + 1) == Some(&b'*') => {
                i = pas_lex::skip_paren_comment(bytes, i + 2)
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = pas_lex::skip_line_comment(bytes, i + 2),
            _ => break,
        }
    }
    i
}

fn scan_entry_tail(bytes: &[u8], mut i: usize) -> (Option<String>, usize, Option<u8>) {
    let mut in_path = None;

    while i < bytes.len() {
        match bytes[i] {
            b',' | b';' => return (in_path, i, Some(bytes[i])),
            b'{' => i = pas_lex::skip_brace_comment(bytes, i + 1),
            b'(' if bytes.get(i + 1) == Some(&b'*') => {
                i = pas_lex::skip_paren_comment(bytes, i + 2)
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = pas_lex::skip_line_comment(bytes, i + 2),
            b'\'' => i = pas_lex::skip_string(bytes, i + 1),
            byte if pas_lex::is_ident_start(byte) => {
                let (token, next) = pas_lex::read_ident(bytes, i);
                if token.eq_ignore_ascii_case("in") {
                    let mut j = skip_ws_and_comments_no_strings(bytes, next);
                    if j < bytes.len() && bytes[j] == b'\'' {
                        if let Some((value, end)) = read_string_literal(bytes, j) {
                            in_path = Some(value);
                            i = end;
                            continue;
                        }
                        j = pas_lex::skip_string(bytes, j + 1);
                    }
                    i = j;
                    continue;
                }
                i = next;
            }
            _ => i += 1,
        }
    }
    (in_path, i, None)
}

fn read_string_literal(bytes: &[u8], start: usize) -> Option<(String, usize)> {
    if bytes.get(start) != Some(&b'\'') {
        return None;
    }
    let mut out = Vec::new();
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => {
                if bytes.get(i + 1) == Some(&b'\'') {
                    out.push(b'\'');
                    i += 2;
                } else {
                    let value = String::from_utf8_lossy(&out).to_string();
                    return Some((value, i + 1));
                }
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    None
}

fn update_separator_flags(
    bytes: &[u8],
    start: usize,
    end: usize,
    has_backslash: &mut bool,
    has_slash: &mut bool,
) {
    let mut i = start;
    let end = end.min(bytes.len());
    while i < end {
        match bytes[i] {
            b'{' => i = pas_lex::skip_brace_comment(bytes, i + 1),
            b'(' if bytes.get(i + 1) == Some(&b'*') => {
                i = pas_lex::skip_paren_comment(bytes, i + 2)
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = pas_lex::skip_line_comment(bytes, i + 2),
            b'\'' => {
                let mut j = i + 1;
                while j < end {
                    match bytes[j] {
                        b'\'' if bytes.get(j + 1) == Some(&b'\'') => {
                            j += 2;
                        }
                        b'\'' => {
                            j += 1;
                            break;
                        }
                        b'\\' => {
                            *has_backslash = true;
                            j += 1;
                        }
                        b'/' => {
                            *has_slash = true;
                            j += 1;
                        }
                        _ => j += 1,
                    }
                }
                i = j;
            }
            _ => i += 1,
        }
    }
}

fn detect_line_ending(bytes: &[u8]) -> &'static str {
    if bytes.windows(2).any(|pair| pair == b"\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, contents)?;
    match fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(path)?;
            fs::rename(temp_path, path)
        }
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parse_dpr_uses_single_line() {
        let src = b"program Demo;\nuses Foo, Bar;\nbegin end.";
        let list = parse_dpr_uses(src).expect("uses list");
        assert_eq!(list.entries.len(), 2);
        assert_eq!(list.entries[0].name, "Foo");
        assert_eq!(list.entries[1].name, "Bar");
        assert!(list.entries[0].in_path.is_none());
        assert!(list.entries[1].in_path.is_none());
        assert!(!list.multiline);
        assert!(list.indent.is_empty());
    }

    #[test]
    fn parse_dpr_uses_multiline_with_indent_and_paths() {
        let src = b"program Demo;\nuses\n  Foo,\n  Bar in 'lib\\Bar.pas',\n  Baz;\nbegin end.";
        let list = parse_dpr_uses(src).expect("uses list");
        assert_eq!(list.entries.len(), 3);
        assert!(list.multiline);
        assert_eq!(list.indent, "  ");
        assert!(
            list.has_backslash,
            "expected backslash path detection, list={list:?}"
        );
        assert!(!list.has_slash);
        assert_eq!(list.entries[1].in_path.as_deref(), Some("lib\\Bar.pas"));
    }

    #[test]
    fn parse_dpr_uses_ignores_comments_and_directives() {
        let src = br#"
program Demo;
uses Foo, {Bar}, (*Baz*), {$IFDEF X} Qux, {$ENDIF} RealUnit;
begin end.
"#;
        let list = parse_dpr_uses(src).expect("uses list");
        let names: Vec<String> = list
            .entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect();
        assert_eq!(names, vec!["Foo", "Qux", "RealUnit"]);
        assert!(list.entries.iter().all(|entry| entry.in_path.is_none()));
    }

    #[test]
    fn insert_new_unit_single_line() {
        let root = temp_dir();
        let dpr_path = root.join("Demo.dpr");
        let pas_path = root.join("NewUnit.pas");
        fs::write(&dpr_path, "program Demo;\nuses Foo, Bar;\nbegin end.").unwrap();
        fs::write(&pas_path, "unit NewUnit;\ninterface\nend.").unwrap();

        let bytes = fs::read(&dpr_path).unwrap();
        let list = parse_dpr_uses(&bytes).expect("uses list");
        let new_unit = UnitFileInfo {
            name: "NewUnit".to_string(),
            path: pas_path.clone(),
            uses: Vec::new(),
        };
        insert_new_unit(&bytes, &dpr_path, &list, &new_unit).unwrap();

        let updated = fs::read_to_string(&dpr_path).unwrap();
        assert!(
            updated.contains("uses Foo, Bar, NewUnit in 'NewUnit.pas';"),
            "{updated}"
        );
    }

    #[test]
    fn insert_new_unit_multiline_keeps_indent_and_separator() {
        let root = temp_dir();
        let dpr_path = root.join("Demo.dpr");
        let pas_dir = root.join("sub");
        fs::create_dir_all(&pas_dir).unwrap();
        let pas_path = pas_dir.join("NewUnit.pas");
        fs::write(
            &dpr_path,
            "program Demo;\r\nuses\r\n  Foo,\r\n  Bar in 'lib/Bar.pas',\r\n  Baz;\r\nbegin end.",
        )
        .unwrap();
        fs::write(&pas_path, "unit NewUnit;\ninterface\nend.").unwrap();

        let bytes = fs::read(&dpr_path).unwrap();
        let list = parse_dpr_uses(&bytes).expect("uses list");
        let new_unit = UnitFileInfo {
            name: "NewUnit".to_string(),
            path: pas_path.clone(),
            uses: Vec::new(),
        };
        insert_new_unit(&bytes, &dpr_path, &list, &new_unit).unwrap();

        let updated = fs::read_to_string(&dpr_path).unwrap();
        assert!(
            updated.contains("Baz,\r\n  NewUnit in 'sub/NewUnit.pas';"),
            "{updated}"
        );
    }

    #[test]
    fn parse_dpr_uses_semicolon_on_own_line() {
        let src = b"program Demo;\nuses\n  Foo,\n  Bar\n;\nbegin end.";
        let list = parse_dpr_uses(src).expect("uses list");
        let names: Vec<String> = list
            .entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect();
        assert_eq!(names, vec!["Foo", "Bar"]);
        assert!(list.multiline);
        assert_eq!(list.indent, "  ");
    }

    #[test]
    fn parse_dpr_uses_mixed_separators_prefers_existing() {
        let src = b"program Demo;\nuses Foo in 'lib/Foo.pas', Bar in 'lib\\\\Bar.pas';\nbegin end.";
        let list = parse_dpr_uses(src).expect("uses list");
        assert!(list.has_slash);
        assert!(list.has_backslash);
    }

    fn temp_dir() -> PathBuf {
        let mut root = env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        root.push(format!("fixdpr_dpr_test_{nanos}"));
        fs::create_dir_all(&root).expect("create temp dir");
        root
    }
}
