use clap::Parser;
use std::path::{Path, PathBuf};
use std::process;

mod dpr_edit;
mod fs_walk;
mod graph;
mod pas_index;
mod pas_lex;

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

    /// Path to a .pas file or a unit name
    #[arg(long, value_name = "VALUE")]
    new_dependency: String,

    /// Optional list of folder prefixes to skip (repeatable or comma-separated)
    #[arg(long, value_name = "PATHS", value_delimiter = ',', action = clap::ArgAction::Append)]
    ignore_paths: Vec<String>,
}

fn main() {
    let cli = Cli::parse();
    if let Err(err) = validate_args(&cli) {
        eprintln!("error: {err}");
        process::exit(2);
    }

    let search_root = fs_walk::canonicalize_root(&cli.search_path);
    let ignore_matcher = fs_walk::build_ignore_matcher(&cli.ignore_paths, &search_root);
    let scan = match fs_walk::scan_files(&search_root, &ignore_matcher) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("error: {err}");
            process::exit(1);
        }
    };
    let index = match pas_index::build_unit_index(&scan.pas_files) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("error: {err}");
            process::exit(1);
        }
    };
    let graph = match graph::build_unit_graph(&index) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("error: {err}");
            process::exit(1);
        }
    };
    let new_unit_id = match graph::resolve_new_unit_id(&cli.new_dependency, &graph, &search_root) {
        Ok(id) => id,
        Err(err) => {
            eprintln!("error: {err}");
            process::exit(1);
        }
    };
    let dependents = graph::compute_dependents(&graph, new_unit_id);
    let dpr_summary =
        match dpr_edit::update_dpr_files(&scan.dpr_files, &graph, new_unit_id, &dependents) {
            Ok(summary) => summary,
            Err(err) => {
                eprintln!("error: {err}");
                process::exit(1);
            }
        };

    let mut warnings = Vec::new();
    warnings.extend(index.warnings.iter().cloned());
    warnings.extend(graph.warnings.iter().cloned());
    warnings.extend(dpr_summary.warnings.iter().cloned());

    println!("Summary:");
    println!("  pas scanned: {}", scan.pas_files.len());
    println!("  dpr scanned: {}", dpr_summary.scanned);
    println!("  dpr updated: {}", dpr_summary.updated);
    if !warnings.is_empty() {
        println!("Warnings:");
        for warning in &warnings {
            println!("  {warning}");
        }
    }

    if dpr_summary.failures > 0 {
        process::exit(1);
    }
}

fn validate_args(cli: &Cli) -> Result<(), String> {
    if !cli.search_path.exists() {
        return Err(format!(
            "--search-path does not exist: {}",
            cli.search_path.display()
        ));
    }
    if !cli.search_path.is_dir() {
        return Err(format!(
            "--search-path is not a directory: {}",
            cli.search_path.display()
        ));
    }

    validate_new_dependency(&cli.new_dependency, &cli.search_path)?;

    Ok(())
}

fn validate_new_dependency(value: &str, search_path: &Path) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("--new-dependency cannot be empty".to_string());
    }

    if is_probably_path(trimmed) {
        let candidate = PathBuf::from(trimmed);
        if candidate.is_file() {
            if !is_pas_file(&candidate) {
                return Err(format!(
                    "--new-dependency must point to a .pas file: {}",
                    candidate.display()
                ));
            }
            return Ok(());
        }
        if candidate.is_relative() {
            let alt = search_path.join(&candidate);
            if alt.is_file() {
                if !is_pas_file(&alt) {
                    return Err(format!(
                        "--new-dependency must point to a .pas file: {}",
                        alt.display()
                    ));
                }
                return Ok(());
            }
        }
        return Err(format!(
            "--new-dependency path not found: {}",
            candidate.display()
        ));
    }

    if is_valid_unit_name(trimmed) {
        return Ok(());
    }

    Err(format!(
        "--new-dependency must be a .pas path or unit name: {trimmed}"
    ))
}

fn is_probably_path(value: &str) -> bool {
    value.contains('/') || value.contains('\\') || value.to_ascii_lowercase().ends_with(".pas")
}

fn is_pas_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("pas"))
        .unwrap_or(false)
}

fn is_valid_unit_name(value: &str) -> bool {
    let mut chars = value.chars();
    let first = match chars.next() {
        Some(ch) => ch,
        None => return false,
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.')
}
