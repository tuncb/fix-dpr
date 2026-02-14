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
        let normalized = normalize_path_for_prefix_match(path);
        self.prefixes
            .iter()
            .any(|prefix| is_prefix(&normalized, prefix))
    }
}

#[derive(Debug, Default)]
pub struct DprIgnoreMatcher {
    patterns: Vec<GlobPattern>,
    normalized_patterns: Vec<String>,
}

impl DprIgnoreMatcher {
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    pub fn normalized_patterns(&self) -> &[String] {
        &self.normalized_patterns
    }

    pub fn is_ignored(&self, absolute_path: &str) -> bool {
        let normalized = normalize_path_like_for_match(absolute_path);
        self.patterns
            .iter()
            .any(|pattern| glob_matches(&pattern.tokens, &normalized))
    }
}

#[derive(Debug, Default)]
pub struct DprFilterResult {
    pub included_files: Vec<PathBuf>,
    pub ignored_files: Vec<PathBuf>,
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
        let normalized = normalize_path_for_prefix_match(&path);
        if !normalized.is_empty() {
            prefixes.push(normalized);
        }
    }

    prefixes.sort();
    prefixes.dedup();

    IgnoreMatcher { prefixes }
}

pub fn build_dpr_ignore_matcher(
    raw_values: &[String],
    cwd: &Path,
) -> Result<DprIgnoreMatcher, String> {
    let mut patterns = Vec::new();
    let mut normalized_patterns = Vec::new();

    for raw in raw_values {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }

        let normalized = normalize_dpr_glob_pattern(trimmed, cwd);
        patterns.push(GlobPattern {
            tokens: parse_glob_tokens(&normalized),
        });
        normalized_patterns.push(normalized);
    }

    Ok(DprIgnoreMatcher {
        patterns,
        normalized_patterns,
    })
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

pub fn filter_ignored_dpr_files(
    dpr_files: &[PathBuf],
    ignore_dpr_matcher: &DprIgnoreMatcher,
) -> DprFilterResult {
    if ignore_dpr_matcher.is_empty() {
        return DprFilterResult {
            included_files: dpr_files.to_vec(),
            ignored_files: Vec::new(),
        };
    }

    let mut included_files = Vec::new();
    let mut ignored_files = Vec::new();

    for path in dpr_files {
        let path_str = path.to_string_lossy();
        if ignore_dpr_matcher.is_ignored(&path_str) {
            ignored_files.push(path.clone());
        } else {
            included_files.push(path.clone());
        }
    }

    DprFilterResult {
        included_files,
        ignored_files,
    }
}

fn normalize_dpr_glob_pattern(raw_pattern: &str, cwd: &Path) -> String {
    let absolute_pattern = if Path::new(raw_pattern).is_absolute() {
        PathBuf::from(raw_pattern)
    } else {
        cwd.join(raw_pattern)
    };
    normalize_path_like_for_match(&absolute_pattern.to_string_lossy())
}

fn normalize_path_like_for_match(value: &str) -> String {
    let normalized = value.replace('\\', "/").to_ascii_lowercase();
    strip_windows_verbatim_prefix(normalized)
}

fn strip_windows_verbatim_prefix(value: String) -> String {
    if let Some(remainder) = value.strip_prefix("//?/unc/") {
        format!("//{remainder}")
    } else if let Some(remainder) = value.strip_prefix("//?/") {
        remainder.to_string()
    } else if let Some(remainder) = value.strip_prefix("//./") {
        remainder.to_string()
    } else {
        value
    }
}

fn canonicalize_if_exists(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn normalize_path_for_prefix_match(path: &Path) -> String {
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

#[derive(Debug)]
struct GlobPattern {
    tokens: Vec<GlobToken>,
}

#[derive(Debug, Clone, Copy)]
enum GlobToken {
    Literal(char),
    Star,
    DoubleStar,
    Question,
}

fn parse_glob_tokens(pattern: &str) -> Vec<GlobToken> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '*' => {
                let mut run = 1;
                while i + run < chars.len() && chars[i + run] == '*' {
                    run += 1;
                }
                if run >= 2 {
                    tokens.push(GlobToken::DoubleStar);
                } else {
                    tokens.push(GlobToken::Star);
                }
                i += run;
            }
            '?' => {
                tokens.push(GlobToken::Question);
                i += 1;
            }
            ch => {
                tokens.push(GlobToken::Literal(ch));
                i += 1;
            }
        }
    }

    tokens
}

fn glob_matches(tokens: &[GlobToken], value: &str) -> bool {
    let value_chars: Vec<char> = value.chars().collect();
    let mut memo = vec![vec![None; value_chars.len() + 1]; tokens.len() + 1];
    glob_matches_from(tokens, &value_chars, 0, 0, &mut memo)
}

fn glob_matches_from(
    tokens: &[GlobToken],
    value: &[char],
    token_idx: usize,
    value_idx: usize,
    memo: &mut [Vec<Option<bool>>],
) -> bool {
    if let Some(cached) = memo[token_idx][value_idx] {
        return cached;
    }

    let matched = if token_idx == tokens.len() {
        value_idx == value.len()
    } else {
        match tokens[token_idx] {
            GlobToken::Literal(expected) => {
                value
                    .get(value_idx)
                    .copied()
                    .map(|ch| ch == expected)
                    .unwrap_or(false)
                    && glob_matches_from(tokens, value, token_idx + 1, value_idx + 1, memo)
            }
            GlobToken::Question => {
                value
                    .get(value_idx)
                    .copied()
                    .map(|ch| ch != '/')
                    .unwrap_or(false)
                    && glob_matches_from(tokens, value, token_idx + 1, value_idx + 1, memo)
            }
            GlobToken::Star => {
                let mut idx = value_idx;
                loop {
                    if glob_matches_from(tokens, value, token_idx + 1, idx, memo) {
                        break true;
                    }
                    let Some(next) = value.get(idx).copied() else {
                        break false;
                    };
                    if next == '/' {
                        break false;
                    }
                    idx += 1;
                }
            }
            GlobToken::DoubleStar => {
                let mut idx = value_idx;
                loop {
                    if glob_matches_from(tokens, value, token_idx + 1, idx, memo) {
                        break true;
                    }
                    if idx == value.len() {
                        break false;
                    }
                    idx += 1;
                }
            }
        }
    };

    memo[token_idx][value_idx] = Some(matched);
    matched
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn build_dpr_ignore_matcher_normalizes_absolute_pattern() {
        let cwd = temp_dir("fixdpr_ignore_abs_");
        let pattern = cwd.join("apps").join("Demo.dpr");
        let matcher = build_dpr_ignore_matcher(&[pattern.to_string_lossy().to_string()], &cwd)
            .expect("matcher");

        let expected = normalize_path_like_for_match(&pattern.to_string_lossy());
        assert_eq!(matcher.normalized_patterns(), &[expected.clone()]);
        assert!(matcher.is_ignored(&pattern.to_string_lossy()));
    }

    #[test]
    fn relative_pattern_is_anchored_to_cwd_as_absolute_pattern() {
        let cwd = temp_dir("fixdpr_ignore_rel_");
        let matcher = build_dpr_ignore_matcher(&["app2/*.dpr".to_string()], &cwd).expect("matcher");

        let candidate = cwd.join("app2").join("App2.dpr");
        assert!(matcher.is_ignored(&candidate.to_string_lossy()));
    }

    #[test]
    fn filter_ignored_dpr_files_matches_absolute_paths() {
        let cwd = temp_dir("fixdpr_ignore_filter_");
        let dpr_a = cwd.join("app1").join("App1.dpr");
        let dpr_b = cwd.join("app2").join("App2.dpr");
        let matcher = build_dpr_ignore_matcher(&["app2/*.dpr".to_string()], &cwd).expect("matcher");

        let filtered = filter_ignored_dpr_files(&[dpr_a.clone(), dpr_b.clone()], &matcher);

        assert_eq!(filtered.included_files, vec![dpr_a]);
        assert_eq!(filtered.ignored_files, vec![dpr_b]);
    }

    #[test]
    fn dpr_glob_matcher_supports_single_and_double_star() {
        let cwd = temp_dir("fixdpr_ignore_glob_");
        let single =
            build_dpr_ignore_matcher(&["app/*.dpr".to_string()], &cwd).expect("single matcher");
        assert!(single.is_ignored(&cwd.join("app").join("Test.dpr").to_string_lossy()));
        assert!(!single.is_ignored(
            &cwd.join("app")
                .join("sub")
                .join("Test.dpr")
                .to_string_lossy()
        ));

        let double =
            build_dpr_ignore_matcher(&["app/**/*.dpr".to_string()], &cwd).expect("double matcher");
        assert!(double.is_ignored(
            &cwd.join("app")
                .join("sub")
                .join("Test.dpr")
                .to_string_lossy()
        ));
    }

    #[cfg(windows)]
    #[test]
    fn build_dpr_ignore_matcher_accepts_cross_drive_absolute_pattern() {
        let cwd = PathBuf::from(r"C:\repo");
        let matcher = build_dpr_ignore_matcher(&[r"D:\repo\App1.dpr".to_string()], &cwd)
            .expect("cross-drive absolute pattern should be accepted");
        assert_eq!(
            matcher.normalized_patterns(),
            &["d:/repo/app1.dpr".to_string()]
        );
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
