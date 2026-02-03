use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::pas_lex;
use crate::uses_include;

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
    let uses = parse_unit_uses(path, &bytes, warnings);
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

pub fn parse_unit_uses(path: &Path, bytes: &[u8], warnings: &mut Vec<String>) -> Vec<String> {
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
                    let mut include_stack = Vec::new();
                    include_stack.push(canonicalize_if_exists(path));
                    let (next_i, _) = parse_uses_fragment_with_includes(
                        bytes,
                        next,
                        path,
                        warnings,
                        &mut deps,
                        &mut include_stack,
                    );
                    i = next_i;
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

fn parse_uses_fragment_with_includes(
    bytes: &[u8],
    mut i: usize,
    source_path: &Path,
    warnings: &mut Vec<String>,
    deps: &mut Vec<String>,
    include_stack: &mut Vec<PathBuf>,
) -> (usize, bool) {
    loop {
        i = skip_ws_comments_and_includes(bytes, i, source_path, warnings, deps, include_stack);
        if i >= bytes.len() {
            return (i, false);
        }
        if bytes[i] == b';' {
            return (i + 1, true);
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

        let (pos, delim) =
            scan_to_delimiter_with_includes(bytes, i, source_path, warnings, deps, include_stack);
        i = pos;
        match delim {
            Some(b',') => i += 1,
            Some(b';') => return (i + 1, true),
            _ => return (i, false),
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

fn scan_to_delimiter_with_includes(
    bytes: &[u8],
    mut i: usize,
    source_path: &Path,
    warnings: &mut Vec<String>,
    deps: &mut Vec<String>,
    include_stack: &mut Vec<PathBuf>,
) -> (usize, Option<u8>) {
    while i < bytes.len() {
        match bytes[i] {
            b',' | b';' => return (i, Some(bytes[i])),
            b'{' | b'(' => {
                if let Some((include_name, end)) = pas_lex::parse_include_directive(bytes, i) {
                    let include_entries = parse_include_entries_for_unit(
                        include_name.as_str(),
                        source_path,
                        warnings,
                        include_stack,
                    );
                    if !include_entries.is_empty() {
                        deps.extend(include_entries);
                    }
                    i = end;
                    continue;
                }
                i = if bytes[i] == b'{' {
                    pas_lex::skip_brace_comment(bytes, i + 1)
                } else if bytes.get(i + 1) == Some(&b'*') {
                    pas_lex::skip_paren_comment(bytes, i + 2)
                } else {
                    i + 1
                };
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = pas_lex::skip_line_comment(bytes, i + 2),
            b'\'' => i = pas_lex::skip_string(bytes, i + 1),
            _ => i += 1,
        }
    }
    (i, None)
}

fn skip_ws_comments_and_includes(
    bytes: &[u8],
    mut i: usize,
    source_path: &Path,
    warnings: &mut Vec<String>,
    deps: &mut Vec<String>,
    include_stack: &mut Vec<PathBuf>,
) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'{' | b'(' => {
                if let Some((include_name, end)) = pas_lex::parse_include_directive(bytes, i) {
                    let include_entries = parse_include_entries_for_unit(
                        include_name.as_str(),
                        source_path,
                        warnings,
                        include_stack,
                    );
                    if !include_entries.is_empty() {
                        deps.extend(include_entries);
                    }
                    i = end;
                    continue;
                }
                i = if bytes[i] == b'{' {
                    pas_lex::skip_brace_comment(bytes, i + 1)
                } else if bytes.get(i + 1) == Some(&b'*') {
                    pas_lex::skip_paren_comment(bytes, i + 2)
                } else {
                    i + 1
                };
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = pas_lex::skip_line_comment(bytes, i + 2),
            b'\'' => i = pas_lex::skip_string(bytes, i + 1),
            _ => break,
        }
    }
    i
}

fn parse_include_entries_for_unit(
    include_name: &str,
    source_path: &Path,
    warnings: &mut Vec<String>,
    include_stack: &mut Vec<PathBuf>,
) -> Vec<String> {
    uses_include::with_include_bytes(
        include_name,
        source_path,
        warnings,
        include_stack,
        |include_path, bytes, warnings, include_stack| {
            let mut entries = Vec::new();
            let _ = parse_uses_fragment_with_includes(
                bytes,
                0,
                include_path,
                warnings,
                &mut entries,
                include_stack,
            );
            entries
        },
    )
    .unwrap_or_default()
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
        let deps = parse_uses_for_test(src);
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
        let deps = parse_uses_for_test(src);
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
        let deps = parse_uses_for_test(src);
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
        let deps = parse_uses_for_test(src);
        assert_eq!(deps, vec!["Foo", "Bar"]);
    }

    #[test]
    fn parse_unit_uses_supports_include_fragments() {
        let root = temp_dir();
        let unit_path = root.join("Demo.pas");
        let include_path = root.join("Uses.inc");
        fs::write(&include_path, "Foo,\nBar in 'lib\\\\Bar.pas',\nBaz,").unwrap();
        let src = br#"
unit Demo;
interface
uses {$I Uses.inc} Qux;
implementation
end.
"#;
        let mut warnings = Vec::new();
        let deps = parse_unit_uses(&unit_path, src, &mut warnings);
        assert_eq!(deps, vec!["Foo", "Bar", "Baz", "Qux"]);
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

    fn parse_uses_for_test(src: &[u8]) -> Vec<String> {
        let root = temp_dir();
        let unit_path = root.join("Demo.pas");
        let mut warnings = Vec::new();
        parse_unit_uses(&unit_path, src, &mut warnings)
    }
}
