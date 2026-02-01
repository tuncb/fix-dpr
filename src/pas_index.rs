use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::pas_lex;

#[derive(Debug, Clone)]
pub struct UnitInfo {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Default)]
pub struct UnitIndex {
    pub units: HashMap<String, UnitInfo>,
    pub ambiguous: HashMap<String, Vec<PathBuf>>,
    pub warnings: Vec<String>,
}

pub fn build_unit_index(paths: &[PathBuf]) -> io::Result<UnitIndex> {
    let mut index = UnitIndex::default();

    for path in paths {
        let content = fs::read(path)?;
        let parsed = parse_unit_name(&content);
        let unit_name = match parsed {
            Some(value) => value,
            None => {
                let fallback = unit_name_from_stem(path);
                if fallback.is_some() {
                    index.warnings.push(format!(
                        "warning: fallback to filename stem for unit name: {}",
                        path.display()
                    ));
                }
                fallback.unwrap_or_default()
            }
        };

        if unit_name.is_empty() {
            index.warnings.push(format!(
                "warning: unable to determine unit name: {}",
                path.display()
            ));
            continue;
        }

        add_unit(&mut index, unit_name, path.clone());
    }

    Ok(index)
}

pub fn derive_unit_name_from_file(path: &Path) -> io::Result<(String, bool)> {
    let content = fs::read(path)?;
    if let Some(name) = parse_unit_name(&content) {
        return Ok((name, false));
    }
    let fallback = unit_name_from_stem(path).unwrap_or_default();
    Ok((fallback, true))
}

fn add_unit(index: &mut UnitIndex, unit_name: String, path: PathBuf) {
    let key = unit_name.to_ascii_lowercase();
    if let Some(existing) = index.units.get(&key) {
        let entry = index
            .ambiguous
            .entry(key.clone())
            .or_insert_with(|| vec![existing.path.clone()]);
        entry.push(path);
        return;
    }

    index.units.insert(
        key,
        UnitInfo {
            name: unit_name,
            path,
        },
    );
}

fn unit_name_from_stem(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_unit_name(bytes: &[u8]) -> Option<String> {
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                i = pas_lex::skip_brace_comment(bytes, i + 1);
            }
            b'(' if bytes.get(i + 1) == Some(&b'*') => {
                i = pas_lex::skip_paren_comment(bytes, i + 2);
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i = pas_lex::skip_line_comment(bytes, i + 2);
            }
            b'\'' => {
                i = pas_lex::skip_string(bytes, i + 1);
            }
            byte if pas_lex::is_ident_start(byte) => {
                let (token, next) = pas_lex::read_ident(bytes, i);
                if token.eq_ignore_ascii_case("unit") {
                    if let Some(name) = parse_unit_name_after(bytes, next) {
                        return Some(name);
                    }
                }
                i = next;
            }
            _ => {
                i += 1;
            }
        }
    }
    None
}

fn parse_unit_name_after(bytes: &[u8], mut i: usize) -> Option<String> {
    i = pas_lex::skip_ws_and_comments(bytes, i);
    if i >= bytes.len() || !pas_lex::is_ident_start(bytes[i]) {
        return None;
    }
    let (name, _) = pas_lex::read_ident_with_dots(bytes, i);
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parse_unit_name_basic() {
        let src = b"unit Foo.Bar;\ninterface\nimplementation\nend.";
        assert_eq!(parse_unit_name(src), Some("Foo.Bar".to_string()));
    }

    #[test]
    fn parse_unit_name_ignores_comments() {
        let src = br#"
{ unit Wrong; }
(* unit AlsoWrong; *)
// unit NoWay;
unit RealUnit;
interface
implementation
end.
"#;
        assert_eq!(parse_unit_name(src), Some("RealUnit".to_string()));
    }

    #[test]
    fn parse_unit_name_ignores_strings() {
        let src = b"const S = 'unit Fake;';\nunit Real;\ninterface\nend.";
        assert_eq!(parse_unit_name(src), Some("Real".to_string()));
    }

    #[test]
    fn parse_unit_name_allows_ifdef_blocks_braces() {
        let src = br#"
{$IFDEF FOO}
{$IFDEF BAR}
{$ENDIF}
{$ENDIF}
unit Real;
"#;
        assert_eq!(parse_unit_name(src), Some("Real".to_string()));
    }

    #[test]
    fn parse_unit_name_allows_ifdef_blocks_paren() {
        let src = br#"
(*$IFDEF FOO*)
unit Conditional;
(*$ENDIF*)
"#;
        assert_eq!(parse_unit_name(src), Some("Conditional".to_string()));
    }

    #[test]
    fn parse_unit_name_allows_nested_ifdefs() {
        let src = br#"
{$IFDEF OUTER}
{$IFNDEF INNER}
unit NestedUnit;
{$ENDIF}
{$ENDIF}
"#;
        assert_eq!(parse_unit_name(src), Some("NestedUnit".to_string()));
    }

    #[test]
    fn parse_unit_name_allows_if_and_ifopt_markers() {
        let src = br#"
{$IFDEF ENABLED}
{$IF 1}
{$IFOPT N+}
unit OptUnit;
{$ENDIF}
{$ENDIF}
{$ENDIF}
"#;
        assert_eq!(parse_unit_name(src), Some("OptUnit".to_string()));
    }

    #[test]
    fn build_unit_index_detects_ambiguous_units() {
        let root = temp_dir();
        let first = write_file(&root, "First.pas", "unit Shared;\ninterface\nend.");
        let second = write_file(&root, "Second.pas", "unit Shared;\ninterface\nend.");
        let index = build_unit_index(&[first.clone(), second.clone()]).expect("index");
        let entry = index.ambiguous.get("shared").expect("expected ambiguity");
        assert_eq!(entry.len(), 2);
        assert!(index.units.contains_key("shared"));
    }

    fn temp_dir() -> PathBuf {
        let mut root = env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        root.push(format!("fixdpr_test_{nanos}"));
        fs::create_dir_all(&root).expect("create temp dir");
        root
    }

    fn write_file(root: &Path, name: &str, contents: &str) -> PathBuf {
        let path = root.join(name);
        fs::write(&path, contents).expect("write file");
        path
    }
}
