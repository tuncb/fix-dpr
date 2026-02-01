use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::graph::{UnitGraph, UnitId};
use crate::pas_lex;

#[derive(Debug)]
pub struct DprUpdateSummary {
    pub scanned: usize,
    pub updated: usize,
    pub warnings: Vec<String>,
    pub failures: usize,
}

#[derive(Debug)]
struct UsesEntry {
    name: String,
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
    graph: &UnitGraph,
    new_unit_id: UnitId,
    dependents: &[bool],
) -> io::Result<DprUpdateSummary> {
    let mut summary = DprUpdateSummary {
        scanned: 0,
        updated: 0,
        warnings: Vec::new(),
        failures: 0,
    };

    let new_unit = match graph.units.get(new_unit_id.0) {
        Some(unit) => unit,
        None => {
            summary
                .warnings
                .push("warning: new unit id is out of bounds".to_string());
            summary.failures += 1;
            return Ok(summary);
        }
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

        if list
            .entries
            .iter()
            .any(|entry| entry.name.eq_ignore_ascii_case(&new_unit.name))
        {
            continue;
        }

        let mut needs_update = false;
        for entry in &list.entries {
            let key = entry.name.to_ascii_lowercase();
            if let Some(id) = graph.name_to_id.get(&key) {
                if dependents.get(id.0).copied().unwrap_or(false) {
                    needs_update = true;
                    break;
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
        }
    }

    Ok(summary)
}

fn insert_new_unit(
    bytes: &[u8],
    dpr_path: &Path,
    list: &UsesList,
    new_unit: &crate::pas_index::UnitInfo,
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

        let (pos, delim) = skip_to_delimiter(bytes, i);
        update_separator_flags(bytes, entry_start, pos, &mut has_backslash, &mut has_slash);
        entries.push(UsesEntry {
            name,
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

fn skip_to_delimiter(bytes: &[u8], mut i: usize) -> (usize, Option<u8>) {
    while i < bytes.len() {
        match bytes[i] {
            b',' | b';' => return (i, Some(bytes[i])),
            b'{' => i = pas_lex::skip_brace_comment(bytes, i + 1),
            b'(' if bytes.get(i + 1) == Some(&b'*') => {
                i = pas_lex::skip_paren_comment(bytes, i + 2)
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = pas_lex::skip_line_comment(bytes, i + 2),
            b'\'' => i = pas_lex::skip_string(bytes, i + 1),
            _ => i += 1,
        }
    }
    (i, None)
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
        let new_unit = crate::pas_index::UnitInfo {
            name: "NewUnit".to_string(),
            path: pas_path.clone(),
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
        let new_unit = crate::pas_index::UnitInfo {
            name: "NewUnit".to_string(),
            path: pas_path.clone(),
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
