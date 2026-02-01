use clap::Parser;
use std::path::{Path, PathBuf};
use std::process;

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
    if let Err(err) = validate_args(cli) {
        eprintln!("error: {err}");
        process::exit(2);
    }
}

fn validate_args(cli: Cli) -> Result<(), String> {
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
    let _ = cli
        .ignore_paths
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .collect::<Vec<_>>();

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
