use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

#[derive(Debug)]
pub struct FsScan {
    pub pas_files: Vec<PathBuf>,
    pub dpr_files: Vec<PathBuf>,
}

#[derive(Debug, Default)]
pub struct IgnoreMatcher {
    prefixes: Vec<String>,
}

impl IgnoreMatcher {
    pub fn is_ignored(&self, path: &Path) -> bool {
        if self.prefixes.is_empty() {
            return false;
        }
        let normalized = normalize_path_for_match(path);
        self.prefixes
            .iter()
            .any(|prefix| is_prefix(&normalized, prefix))
    }
}

pub fn canonicalize_root(root: &Path) -> PathBuf {
    canonicalize_if_exists(root)
}

pub fn build_ignore_matcher(raw_values: &[String], search_root: &Path) -> IgnoreMatcher {
    let mut prefixes = Vec::new();
    for raw in raw_values {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }

        let mut path = PathBuf::from(trimmed);
        if path.is_relative() {
            path = search_root.join(path);
        }
        let path = canonicalize_if_exists(&path);
        let normalized = normalize_path_for_match(&path);
        if !normalized.is_empty() {
            prefixes.push(normalized);
        }
    }

    prefixes.sort();
    prefixes.dedup();

    IgnoreMatcher { prefixes }
}

pub fn scan_files(search_root: &Path, ignore: &IgnoreMatcher) -> io::Result<FsScan> {
    let mut pas_files = Vec::new();
    let mut dpr_files = Vec::new();

    let walker = WalkDir::new(search_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !ignore.is_ignored(entry.path()));

    for entry in walker {
        let entry = match entry {
            Ok(value) => value,
            Err(err) => {
                return Err(io::Error::other(err));
            }
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        if ignore.is_ignored(path) {
            continue;
        }

        if has_extension(path, "pas") {
            pas_files.push(path.to_path_buf());
        } else if has_extension(path, "dpr") {
            dpr_files.push(path.to_path_buf());
        }
    }

    Ok(FsScan {
        pas_files,
        dpr_files,
    })
}

fn canonicalize_if_exists(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn normalize_path_for_match(path: &Path) -> String {
    let replaced = path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    trim_trailing_separators(replaced)
}

fn trim_trailing_separators(mut value: String) -> String {
    while value.ends_with('\\') && value.len() > 2 {
        value.pop();
    }
    value
}

fn is_prefix(path: &str, prefix: &str) -> bool {
    if prefix.is_empty() || path.len() < prefix.len() {
        return false;
    }
    if !path.starts_with(prefix) {
        return false;
    }
    if path.len() == prefix.len() {
        return true;
    }
    path.as_bytes()
        .get(prefix.len())
        .copied()
        .map(|value| value == b'\\')
        .unwrap_or(false)
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case(extension))
        .unwrap_or(false)
}
