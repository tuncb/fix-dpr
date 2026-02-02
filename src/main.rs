use clap::Parser;
use pathdiff::diff_paths;
use std::env;
use std::path::{Path, PathBuf};
use std::process;

mod dpr_edit;
mod fs_walk;
mod pas_lex;
mod unit_cache;

#[derive(Parser, Debug)]
#[command(
    name = "fixdpr",
    version,
    about = "Update Delphi .dpr files to add missing unit dependencies",
    arg_required_else_help = true
)]
struct Cli {
    /// Root folder to recursively scan for .dpr and .pas
    #[arg(long, value_name = "PATH")]
    search_path: PathBuf,

    /// Path to a .pas file (absolute or relative to the current directory)
    #[arg(long, value_name = "VALUE")]
    new_dependency: String,

    /// Optional list of folder prefixes to skip (repeatable or comma-separated)
    #[arg(long, value_name = "PATHS", value_delimiter = ',', action = clap::ArgAction::Append)]
    ignore_paths: Vec<String>,
}

fn main() {
    let cli = Cli::parse();
    if let Err(err) = validate_search_path(&cli.search_path) {
        eprintln!("error: {err}");
        process::exit(2);
    }
    let new_dependency_path = match resolve_new_dependency_path(&cli.new_dependency) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("error: {err}");
            process::exit(2);
        }
    };
    if let Err(err) = validate_new_dependency_path(&new_dependency_path) {
        eprintln!("error: {err}");
        process::exit(2);
    }

    let search_root = fs_walk::canonicalize_root(&cli.search_path);
    let ignore_matcher = fs_walk::build_ignore_matcher(&cli.ignore_paths, &search_root);
    println!("fixdpr {}", env!("CARGO_PKG_VERSION"));
    println!("Scanning {}", search_root.display());
    let ignore_display = format_ignore_paths(&cli.ignore_paths);
    if !ignore_display.is_empty() {
        println!("Ignoring: {}", ignore_display);
    }
    let scan = match fs_walk::scan_files(&search_root, &ignore_matcher) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("error: {err}");
            process::exit(1);
        }
    };
    println!(
        "Found {} .pas, {} .dpr",
        scan.pas_files.len(),
        scan.dpr_files.len()
    );
    let mut warnings = Vec::new();
    println!("Building unit cache...");
    let mut unit_cache = match unit_cache::build_unit_cache(&scan.pas_files, &mut warnings) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("error: {err}");
            process::exit(1);
        }
    };
    println!("Unit cache ready ({} units)", scan.pas_files.len());
    let new_dependency_path = unit_cache::canonicalize_if_exists(&new_dependency_path);
    let new_unit = match unit_cache::load_unit_file(&new_dependency_path, &mut warnings) {
        Ok(Some(unit)) => unit,
        Ok(None) => {
            eprintln!(
                "error: unable to determine unit name from new dependency: {}",
                new_dependency_path.display()
            );
            process::exit(1);
        }
        Err(err) => {
            eprintln!("error: {err}");
            process::exit(1);
        }
    };
    println!(
        "New dependency: {} ({})",
        new_unit.name,
        new_unit.path.display()
    );
    println!("Updating .dpr files... {}", scan.dpr_files.len());
    let dpr_summary = match dpr_edit::update_dpr_files(&scan.dpr_files, &mut unit_cache, &new_unit)
    {
        Ok(summary) => summary,
        Err(err) => {
            eprintln!("error: {err}");
            process::exit(1);
        }
    };
    warnings.extend(dpr_summary.warnings.iter().cloned());

    let unchanged = dpr_summary
        .scanned
        .saturating_sub(dpr_summary.updated)
        .saturating_sub(dpr_summary.failures);

    println!();
    println!("Report:");
    println!("  pas scanned: {}", scan.pas_files.len());
    println!("  dpr scanned: {}", dpr_summary.scanned);
    println!("  dpr updated: {}", dpr_summary.updated);
    println!("  dpr unchanged: {}", unchanged);
    println!("  dpr failures: {}", dpr_summary.failures);
    println!("Updated dpr files ({}):", dpr_summary.updated);
    if dpr_summary.updated_paths.is_empty() {
        println!("  (none)");
    } else {
        for path in &dpr_summary.updated_paths {
            println!("  {}", display_path(path, &search_root));
        }
    }
    if !warnings.is_empty() {
        println!("Warnings ({}):", warnings.len());
        for warning in &warnings {
            println!("  {warning}");
        }
    }

    if dpr_summary.failures > 0 {
        process::exit(1);
    }
}

fn validate_search_path(search_path: &Path) -> Result<(), String> {
    if !search_path.exists() {
        return Err(format!(
            "--search-path does not exist: {}",
            search_path.display()
        ));
    }
    if !search_path.is_dir() {
        return Err(format!(
            "--search-path is not a directory: {}",
            search_path.display()
        ));
    }

    Ok(())
}

fn resolve_new_dependency_path(value: &str) -> Result<PathBuf, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("--new-dependency cannot be empty".to_string());
    }

    let mut path = PathBuf::from(trimmed);
    if path.is_relative() {
        let cwd =
            env::current_dir().map_err(|err| format!("failed to read current directory: {err}"))?;
        path = cwd.join(path);
    }
    Ok(path)
}

fn is_pas_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("pas"))
        .unwrap_or(false)
}

fn validate_new_dependency_path(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!(
            "--new-dependency path not found: {}",
            path.display()
        ));
    }
    if !is_pas_file(path) {
        return Err(format!(
            "--new-dependency must point to a .pas file: {}",
            path.display()
        ));
    }
    Ok(())
}

fn format_ignore_paths(values: &[String]) -> String {
    let mut entries = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        entries.push(trimmed.to_string());
    }
    entries.join(", ")
}

fn display_path(path: &Path, root: &Path) -> String {
    diff_paths(path, root)
        .unwrap_or_else(|| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}
