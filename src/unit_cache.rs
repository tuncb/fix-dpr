use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::pas_lex;

#[derive(Debug, Clone)]
pub struct UnitFileInfo {
    pub name: String,
    pub path: PathBuf,
    pub uses: Vec<String>,
}

#[derive(Debug, Default)]
pub struct UnitCache {
    pub by_path: HashMap<PathBuf, UnitFileInfo>,
    pub by_name: HashMap<String, Vec<PathBuf>>,
}

pub fn build_unit_cache(paths: &[PathBuf], warnings: &mut Vec<String>) -> io::Result<UnitCache> {
    let mut cache = UnitCache::default();

    for path in paths {
        let canonical = canonicalize_if_exists(path);
        if cache.by_path.contains_key(&canonical) {
            continue;
        }
        if let Some(info) = load_unit_file(&canonical, warnings)? {
            insert_unit(&mut cache, canonical, info);
        }
    }

    Ok(cache)
}

pub fn get_or_load<'a>(
    cache: &'a mut UnitCache,
    path: &Path,
    warnings: &mut Vec<String>,
) -> io::Result<Option<&'a UnitFileInfo>> {
    let canonical = canonicalize_if_exists(path);
    if cache.by_path.contains_key(&canonical) {
        return Ok(cache.by_path.get(&canonical));
    }
    if let Some(info) = load_unit_file(&canonical, warnings)? {
        insert_unit(cache, canonical.clone(), info);
        return Ok(cache.by_path.get(&canonical));
    }
    Ok(None)
}

pub fn load_unit_file(path: &Path, warnings: &mut Vec<String>) -> io::Result<Option<UnitFileInfo>> {
    let bytes = fs::read(path)?;
    let name = match determine_unit_name(path, &bytes, warnings) {
        Some(value) => value,
        None => return Ok(None),
    };
    let uses = parse_unit_uses(&bytes);
    Ok(Some(UnitFileInfo {
        name,
        path: path.to_path_buf(),
        uses,
    }))
}

fn insert_unit(cache: &mut UnitCache, path: PathBuf, info: UnitFileInfo) {
    let key = info.name.to_ascii_lowercase();
    cache.by_path.insert(path.clone(), info);
    cache.by_name.entry(key).or_default().push(path);
}

pub fn canonicalize_if_exists(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn determine_unit_name(path: &Path, bytes: &[u8], warnings: &mut Vec<String>) -> Option<String> {
    if let Some(value) = parse_unit_name(bytes) {
        return Some(value);
    }

    let fallback = unit_name_from_stem(path);
    if let Some(value) = fallback {
        warnings.push(format!(
            "warning: fallback to filename stem for unit name: {}",
            path.display()
        ));
        return Some(value);
    }

    warnings.push(format!(
        "warning: unable to determine unit name: {}",
        path.display()
    ));
    None
}

fn unit_name_from_stem(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn parse_unit_name(bytes: &[u8]) -> Option<String> {
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Section {
    None,
    Interface,
    Implementation,
}

pub fn parse_unit_uses(bytes: &[u8]) -> Vec<String> {
    let mut deps = Vec::new();
    let mut i = 0;
    let mut section = Section::None;

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
                if token.eq_ignore_ascii_case("interface") {
                    section = Section::Interface;
                } else if token.eq_ignore_ascii_case("implementation") {
                    section = Section::Implementation;
                } else if token.eq_ignore_ascii_case("uses") && section != Section::None {
                    i = parse_uses_list(bytes, next, &mut deps);
                    continue;
                }
                i = next;
            }
            _ => {
                i += 1;
            }
        }
    }

    deps
}

fn parse_uses_list(bytes: &[u8], mut i: usize, deps: &mut Vec<String>) -> usize {
    loop {
        i = pas_lex::skip_ws_and_comments(bytes, i);
        if i >= bytes.len() {
            return i;
        }
        if bytes[i] == b';' {
            return i + 1;
        }
        if !pas_lex::is_ident_start(bytes[i]) {
            i += 1;
            continue;
        }
        let (name, next) = pas_lex::read_ident_with_dots(bytes, i);
        if !name.is_empty() {
            deps.push(name);
        }
        i = next;
        i = pas_lex::skip_ws_and_comments(bytes, i);

        if let Some((token, next_token)) = peek_ident(bytes, i) {
            if token.eq_ignore_ascii_case("in") {
                i = next_token;
                i = pas_lex::skip_ws_and_comments(bytes, i);
                if i < bytes.len() && bytes[i] == b'\'' {
                    i = pas_lex::skip_string(bytes, i + 1);
                }
            }
        }

        let (pos, delim) = skip_to_delimiter(bytes, i);
        i = pos;
        match delim {
            Some(b',') => i += 1,
            Some(b';') => return i + 1,
            _ => return i,
        }
    }
}

fn peek_ident(bytes: &[u8], i: usize) -> Option<(String, usize)> {
    if i < bytes.len() && pas_lex::is_ident_start(bytes[i]) {
        let (token, next) = pas_lex::read_ident(bytes, i);
        return Some((token, next));
    }
    None
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
    fn parse_unit_uses_in_interface_and_implementation() {
        let src = br#"
unit Demo;
interface
uses Foo, Bar;
implementation
uses Baz;
end.
"#;
        let deps = parse_unit_uses(src);
        assert_eq!(deps, vec!["Foo", "Bar", "Baz"]);
    }

    #[test]
    fn parse_unit_uses_ignores_comments_and_strings() {
        let src = br#"
unit Demo;
interface
uses Foo, {Bar}, (*Baz*), // Quux
  'NotAUnit', RealUnit;
implementation
uses ImplUnit;
end.
"#;
        let deps = parse_unit_uses(src);
        assert_eq!(deps, vec!["Foo", "RealUnit", "ImplUnit"]);
    }

    #[test]
    fn parse_unit_uses_allows_directives_inside_list() {
        let src = br#"
unit Demo;
interface
uses Foo, {$IFDEF X} Bar, {$ENDIF} Baz;
implementation
uses Qux;
end.
"#;
        let deps = parse_unit_uses(src);
        assert_eq!(deps, vec!["Foo", "Bar", "Baz", "Qux"]);
    }

    #[test]
    fn parse_unit_uses_with_in_paths() {
        let src = br#"
unit Demo;
interface
uses Foo in 'Foo.pas', Bar in 'path\Bar.pas';
implementation
end.
"#;
        let deps = parse_unit_uses(src);
        assert_eq!(deps, vec!["Foo", "Bar"]);
    }

    #[test]
    fn load_unit_file_uses_fallback_name() {
        let root = temp_dir();
        let path = root.join("Fallback.pas");
        fs::write(&path, "const X = 1;").unwrap();
        let mut warnings = Vec::new();
        let info = load_unit_file(&path, &mut warnings).unwrap().expect("unit");
        assert_eq!(info.name, "Fallback");
        assert!(!warnings.is_empty());
    }

    fn temp_dir() -> PathBuf {
        let mut root = env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        root.push(format!("fixdpr_unit_cache_{nanos}"));
        fs::create_dir_all(&root).expect("create temp dir");
        root
    }
}
