use std::collections::HashSet;
use std::path::{Path, PathBuf};

const SOURCE_DIR_NAME: &str = "source";

pub fn resolve_source_roots(raw_versions: &[String]) -> Result<Vec<PathBuf>, String> {
    #[cfg(windows)]
    {
        resolve_source_roots_with_lookup(raw_versions, lookup_bds_root_from_registry)
    }

    #[cfg(not(windows))]
    {
        let has_any = raw_versions.iter().any(|value| !value.trim().is_empty());
        if has_any {
            return Err("--delphi-version is only supported on Windows".to_string());
        }
        Ok(Vec::new())
    }
}

fn resolve_source_roots_with_lookup<F>(
    raw_versions: &[String],
    mut lookup_bds_root: F,
) -> Result<Vec<PathBuf>, String>
where
    F: FnMut(&str) -> Result<Option<PathBuf>, String>,
{
    let mut roots = Vec::new();
    let mut seen = HashSet::new();

    for raw in raw_versions {
        let version = raw.trim();
        if version.is_empty() {
            continue;
        }

        let bds_root = match lookup_bds_root(version)? {
            Some(path) => path,
            None => {
                return Err(format!("--delphi-version not found in registry: {version}"));
            }
        };

        let source_root = bds_root.join(SOURCE_DIR_NAME);
        if !source_root.exists() {
            return Err(format!(
                "Delphi source path not found for --delphi-version {version}: {}",
                source_root.display()
            ));
        }
        if !source_root.is_dir() {
            return Err(format!(
                "Delphi source path is not a directory for --delphi-version {version}: {}",
                source_root.display()
            ));
        }

        let canonical = canonicalize_if_exists(&source_root);
        let dedupe_key = normalize_for_dedupe(&canonical);
        if seen.insert(dedupe_key) {
            roots.push(canonical);
        }
    }

    roots.sort_by_key(|path| normalize_for_dedupe(path.as_path()));
    Ok(roots)
}

#[cfg(windows)]
fn lookup_bds_root_from_registry(version: &str) -> Result<Option<PathBuf>, String> {
    let candidates = version_candidates(version);
    if candidates.is_empty() {
        return Ok(None);
    }

    let registry_bases = [
        r"HKCU\Software\Embarcadero\BDS",
        r"HKLM\Software\Embarcadero\BDS",
        r"HKLM\Software\WOW6432Node\Embarcadero\BDS",
    ];

    for candidate in candidates {
        for base in registry_bases {
            let key_path = format!(r"{base}\{candidate}");
            let root_dir = query_registry_value(&key_path, "RootDir")
                .map_err(|err| format!("failed to query registry key {key_path}: {err}"))?;
            let Some(root_dir) = root_dir else {
                continue;
            };
            let trimmed = root_dir.trim().trim_matches('"');
            if trimmed.is_empty() {
                continue;
            }
            return Ok(Some(PathBuf::from(trimmed)));
        }
    }

    Ok(None)
}

#[cfg(windows)]
fn query_registry_value(key_path: &str, value_name: &str) -> std::io::Result<Option<String>> {
    let output = std::process::Command::new("reg")
        .args(["query", key_path, "/v", value_name])
        .output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_reg_query_value(&stdout, value_name))
}

fn parse_reg_query_value(output: &str, value_name: &str) -> Option<String> {
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let mut parts = trimmed.split_whitespace();
        let Some(name) = parts.next() else {
            continue;
        };
        if !name.eq_ignore_ascii_case(value_name) {
            continue;
        }

        let _value_type = parts.next()?;
        let value = parts.collect::<Vec<_>>().join(" ");
        if value.is_empty() {
            continue;
        }

        return Some(value);
    }

    None
}

fn version_candidates(version: &str) -> Vec<String> {
    let trimmed = version.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    candidates.push(trimmed.to_string());

    if !trimmed.contains('.') {
        candidates.push(format!("{trimmed}.0"));
    }

    if let Some(base) = trimmed.strip_suffix(".0") {
        if !base.is_empty() {
            candidates.push(base.to_string());
        }
    }

    let mut seen = HashSet::new();
    candidates.retain(|candidate| seen.insert(candidate.to_ascii_lowercase()));
    candidates
}

fn canonicalize_if_exists(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn normalize_for_dedupe(path: &Path) -> String {
    let mut normalized = path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    while normalized.ends_with('\\') && normalized.len() > 2 {
        normalized.pop();
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::env;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn version_candidates_accept_short_and_long_forms() {
        assert_eq!(version_candidates("22"), vec!["22", "22.0"]);
        assert_eq!(version_candidates("22.0"), vec!["22.0", "22"]);
    }

    #[test]
    fn parse_reg_query_value_extracts_root_dir() {
        let output = r#"
HKEY_CURRENT_USER\Software\Embarcadero\BDS\22.0
    RootDir    REG_SZ    C:\Program Files (x86)\Embarcadero\Studio\22.0\
"#;
        let value = parse_reg_query_value(output, "RootDir");
        assert_eq!(
            value.as_deref(),
            Some(r"C:\Program Files (x86)\Embarcadero\Studio\22.0\")
        );
    }

    #[test]
    fn resolve_source_roots_with_lookup_builds_source_paths() {
        let root = temp_dir("fixdpr_delphi_resolve_ok_");
        let v22 = root.join("bds22");
        let v23 = root.join("bds23");
        fs::create_dir_all(v22.join("source")).expect("create bds22 source");
        fs::create_dir_all(v23.join("source")).expect("create bds23 source");

        let mut lookup = HashMap::new();
        lookup.insert("22".to_string(), v22.clone());
        lookup.insert("23.0".to_string(), v23.clone());

        let versions = vec!["22".to_string(), "23.0".to_string()];
        let roots =
            resolve_source_roots_with_lookup(&versions, |version| Ok(lookup.get(version).cloned()))
                .expect("resolve roots");
        let bds22_source = PathBuf::from("bds22").join(SOURCE_DIR_NAME);
        let bds23_source = PathBuf::from("bds23").join(SOURCE_DIR_NAME);

        assert_eq!(roots.len(), 2);
        assert!(roots.iter().any(|path| path.ends_with(&bds22_source)));
        assert!(roots.iter().any(|path| path.ends_with(&bds23_source)));
    }

    #[test]
    fn resolve_source_roots_with_lookup_requires_existing_source_dir() {
        let root = temp_dir("fixdpr_delphi_resolve_missing_");
        let v22 = root.join("bds22");
        fs::create_dir_all(&v22).expect("create bds22 root");

        let versions = vec!["22".to_string()];
        let err = resolve_source_roots_with_lookup(&versions, |_version| Ok(Some(v22.clone())))
            .expect_err("expected missing source error");
        assert!(err.contains("Delphi source path not found"), "{err}");
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let mut root = env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        root.push(format!("{prefix}{nanos}"));
        root
    }
}
