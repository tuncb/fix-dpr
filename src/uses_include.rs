use std::fs;
use std::path::{Path, PathBuf};

pub fn with_include_bytes<T, F>(
    include_name: &str,
    source_path: &Path,
    warnings: &mut Vec<String>,
    include_stack: &mut Vec<PathBuf>,
    f: F,
) -> Option<T>
where
    F: FnOnce(&Path, &[u8], &mut Vec<String>, &mut Vec<PathBuf>) -> T,
{
    let include_path = resolve_include_path(source_path, include_name);
    let canonical = canonicalize_if_exists(&include_path);
    if include_stack.contains(&canonical) {
        warnings.push(format!(
            "warning: include cycle detected for {} (from {})",
            include_path.display(),
            source_path.display()
        ));
        return None;
    }
    let bytes = match fs::read(&include_path) {
        Ok(data) => data,
        Err(err) => {
            warnings.push(format!(
                "warning: failed to read include {} referenced by {}: {err}",
                include_path.display(),
                source_path.display()
            ));
            return None;
        }
    };

    include_stack.push(canonical);
    let result = f(&include_path, &bytes, warnings, include_stack);
    include_stack.pop();
    Some(result)
}

pub fn resolve_include_path(source_path: &Path, include: &str) -> PathBuf {
    let candidate = PathBuf::from(include);
    if candidate.is_absolute() {
        candidate
    } else {
        source_path
            .parent()
            .map(|parent| parent.join(&candidate))
            .unwrap_or(candidate)
    }
}

fn canonicalize_if_exists(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
