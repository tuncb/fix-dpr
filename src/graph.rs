use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::Path;

use crate::pas_index::{self, UnitIndex, UnitInfo};
use crate::pas_lex;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct UnitId(pub usize);

#[derive(Debug)]
pub struct UnitGraph {
    pub units: Vec<UnitInfo>,
    pub deps: Vec<Vec<UnitId>>,
    pub rev: Vec<Vec<UnitId>>,
    pub name_to_id: HashMap<String, UnitId>,
    pub warnings: Vec<String>,
}

pub fn build_unit_graph(index: &UnitIndex) -> io::Result<UnitGraph> {
    let mut entries: Vec<(&String, &UnitInfo)> = index.units.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    let mut units = Vec::with_capacity(entries.len());
    let mut name_to_id = HashMap::with_capacity(entries.len());
    for (idx, (key, info)) in entries.iter().enumerate() {
        let id = UnitId(idx);
        name_to_id.insert((*key).clone(), id);
        units.push((*info).clone());
    }

    let mut deps = vec![Vec::new(); units.len()];
    let mut warnings = Vec::new();
    for (idx, info) in units.iter().enumerate() {
        let bytes = fs::read(&info.path)?;
        let parsed = parse_unit_uses(&bytes);
        let mut seen = HashSet::new();
        for dep in parsed {
            let key = dep.to_ascii_lowercase();
            if index.ambiguous.contains_key(&key) {
                warnings.push(format!(
                    "warning: ambiguous unit reference '{dep}' in {}",
                    info.path.display()
                ));
            }
            let Some(dep_id) = name_to_id.get(&key).copied() else {
                continue;
            };
            if seen.insert(dep_id.0) {
                deps[idx].push(dep_id);
            }
        }
    }

    let mut rev = vec![Vec::new(); units.len()];
    for (source, deps) in deps.iter().enumerate() {
        for target in deps {
            rev[target.0].push(UnitId(source));
        }
    }

    Ok(UnitGraph {
        units,
        deps,
        rev,
        name_to_id,
        warnings,
    })
}

pub fn compute_dependents(graph: &UnitGraph, root: UnitId) -> Vec<bool> {
    let mut visited = vec![false; graph.units.len()];
    let mut queue = VecDeque::new();
    visited[root.0] = true;
    queue.push_back(root);

    while let Some(current) = queue.pop_front() {
        for next in &graph.rev[current.0] {
            if !visited[next.0] {
                visited[next.0] = true;
                queue.push_back(*next);
            }
        }
    }

    visited
}

pub fn resolve_new_unit_id(
    new_dependency: &str,
    graph: &UnitGraph,
    search_root: &Path,
) -> Result<UnitId, String> {
    let trimmed = new_dependency.trim();
    if trimmed.is_empty() {
        return Err("--new-dependency cannot be empty".to_string());
    }

    if is_probably_path(trimmed) {
        let path = resolve_candidate_path(trimmed, search_root)
            .ok_or_else(|| format!("--new-dependency path not found: {trimmed}"))?;
        let (name, _) = pas_index::derive_unit_name_from_file(&path)
            .map_err(|err| format!("failed to read new dependency: {err}"))?;
        if name.is_empty() {
            return Err(format!(
                "unable to derive unit name from new dependency: {}",
                path.display()
            ));
        }
        let key = name.to_ascii_lowercase();
        return graph.name_to_id.get(&key).copied().ok_or_else(|| {
            format!(
                "new dependency unit not found in index: {name} (from {})",
                path.display()
            )
        });
    }

    let key = trimmed.to_ascii_lowercase();
    graph
        .name_to_id
        .get(&key)
        .copied()
        .ok_or_else(|| format!("new dependency unit not found in index: {trimmed}"))
}

fn resolve_candidate_path(value: &str, search_root: &Path) -> Option<std::path::PathBuf> {
    let candidate = std::path::PathBuf::from(value);
    if candidate.is_file() {
        return Some(candidate);
    }
    if candidate.is_relative() {
        let alt = search_root.join(&candidate);
        if alt.is_file() {
            return Some(alt);
        }
    }
    None
}

fn is_probably_path(value: &str) -> bool {
    value.contains('/') || value.contains('\\') || value.to_ascii_lowercase().ends_with(".pas")
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Section {
    None,
    Interface,
    Implementation,
}

fn parse_unit_uses(bytes: &[u8]) -> Vec<String> {
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
}
