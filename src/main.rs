use clap::{ArgGroup, Args, Parser, Subcommand};
use pathdiff::diff_paths;
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process;

mod conditionals;
mod delphi;
mod dpr_edit;
mod fs_walk;
mod pas_lex;
mod unit_cache;
mod uses_include;

#[derive(Parser, Debug)]
#[command(
    name = "fixdpr",
    version,
    about = "Update Delphi .dpr files to add missing unit dependencies",
    arg_required_else_help = true,
    subcommand_required = true
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Add a specific new dependency to matching .dpr files
    AddDependency(AddDependencyArgs),
    /// Insert a specific dependency into selected .dpr files regardless of current usage
    InsertDependency(InsertDependencyArgs),
    /// Remove a specific dependency from selected .dpr files and cascade orphan cleanup
    DeleteDependency(DeleteDependencyArgs),
    /// Fix a single .dpr file by adding missing dependencies in its uses chain
    FixDpr(FixDprArgs),
    /// List conditional unit dependencies for a single .dpr file
    ListConditionals(ListConditionalsArgs),
}

#[derive(Args, Debug)]
struct AddDependencyArgs {
    #[command(flatten)]
    common: SharedArgs,

    #[command(flatten)]
    dependency_lookup: DependencyLookupArgs,

    #[command(flatten)]
    dpr_filter: AddDependencyDprFilterArgs,

    /// Optional Delphi/VCL source root path to scan for fallback unit resolution (repeatable)
    #[arg(long, value_name = "PATH", action = clap::ArgAction::Append)]
    delphi_path: Vec<String>,

    /// Optional Delphi version to resolve from registry and use as fallback source root (repeatable)
    #[arg(long, value_name = "VERSION", action = clap::ArgAction::Append)]
    delphi_version: Vec<String>,

    /// Path to a .pas file (absolute or relative to the current directory)
    #[arg(value_name = "NEW_DEPENDENCY")]
    new_dependency: String,

    /// Disable adding transitive dependencies introduced by NEW_DEPENDENCY
    #[arg(long)]
    disable_introduced_dependencies: bool,

    /// Run a follow-up fix pass on each dpr updated by add-dependency
    #[arg(long)]
    fix_updated_dprs: bool,
}

#[derive(Args, Debug)]
struct InsertDependencyArgs {
    #[command(flatten)]
    common: SharedArgs,

    #[command(flatten)]
    dependency_lookup: DependencyLookupArgs,

    #[command(flatten)]
    dpr_filter: AddDependencyDprFilterArgs,

    #[command(flatten)]
    targets: InsertDependencyTargetArgs,

    /// Optional Delphi/VCL source root path to scan for fallback unit resolution (repeatable)
    #[arg(long, value_name = "PATH", action = clap::ArgAction::Append)]
    delphi_path: Vec<String>,

    /// Optional Delphi version to resolve from registry and use as fallback source root (repeatable)
    #[arg(long, value_name = "VERSION", action = clap::ArgAction::Append)]
    delphi_version: Vec<String>,

    /// Path to a .pas file (absolute or relative to the current directory)
    #[arg(value_name = "NEW_DEPENDENCY")]
    new_dependency: String,

    /// Disable adding transitive dependencies introduced by NEW_DEPENDENCY
    #[arg(long)]
    disable_introduced_dependencies: bool,
}

#[derive(Args, Debug)]
struct DeleteDependencyArgs {
    #[command(flatten)]
    common: SharedArgs,

    #[command(flatten)]
    dependency_lookup: DependencyLookupArgs,

    #[command(flatten)]
    dpr_filter: AddDependencyDprFilterArgs,

    #[command(flatten)]
    targets: InsertDependencyTargetArgs,

    /// Optional Delphi/VCL source root path to scan for fallback unit resolution (repeatable)
    #[arg(long, value_name = "PATH", action = clap::ArgAction::Append)]
    delphi_path: Vec<String>,

    /// Optional Delphi version to resolve from registry and use as fallback source root (repeatable)
    #[arg(long, value_name = "VERSION", action = clap::ArgAction::Append)]
    delphi_version: Vec<String>,

    /// Path to a .pas file (absolute or relative to the current directory)
    #[arg(value_name = "OLD_DEPENDENCY")]
    old_dependency: String,
}

#[derive(Args, Debug)]
struct FixDprArgs {
    #[command(flatten)]
    common: SharedArgs,

    #[command(flatten)]
    dependency_lookup: DependencyLookupArgs,

    /// Optional Delphi/VCL source root path to scan for fallback unit resolution (repeatable)
    #[arg(long, value_name = "PATH", action = clap::ArgAction::Append)]
    delphi_path: Vec<String>,

    /// Optional Delphi version to resolve from registry and use as fallback source root (repeatable)
    #[arg(long, value_name = "VERSION", action = clap::ArgAction::Append)]
    delphi_version: Vec<String>,

    /// Path to the target .dpr file to repair (absolute or relative to the current directory)
    #[arg(value_name = "DPR_FILE")]
    dpr_file: String,
}

#[derive(Args, Debug)]
struct ListConditionalsArgs {
    #[command(flatten)]
    common: SharedArgs,

    #[command(flatten)]
    dependency_lookup: DependencyLookupArgs,

    /// Optional Delphi/VCL source root path to scan for fallback unit resolution (repeatable)
    #[arg(long, value_name = "PATH", action = clap::ArgAction::Append)]
    delphi_path: Vec<String>,

    /// Optional Delphi version to resolve from registry and use as fallback source root (repeatable)
    #[arg(long, value_name = "VERSION", action = clap::ArgAction::Append)]
    delphi_version: Vec<String>,

    /// Path to the target .dpr file to inspect (absolute or relative to the current directory)
    #[arg(value_name = "DPR_FILE")]
    dpr_file: String,
}

#[derive(Args, Debug)]
struct SharedArgs {
    /// Root folder path to recursively scan for .dpr and .pas (repeatable)
    #[arg(long, value_name = "PATH", action = clap::ArgAction::Append)]
    search_path: Vec<String>,

    /// Optional folder path to skip recursively (repeatable)
    #[arg(long, value_name = "PATH", action = clap::ArgAction::Append)]
    ignore_path: Vec<String>,

    /// Show detailed info list
    #[arg(long)]
    show_infos: bool,

    /// Show detailed warnings list
    #[arg(long)]
    show_warnings: bool,
}

#[derive(Args, Debug, Default)]
struct DependencyLookupArgs {
    /// Assume compiler symbol is undefined during dependency traversal (repeatable)
    #[arg(long, value_name = "SYMBOL", action = clap::ArgAction::Append)]
    assume_off: Vec<String>,
}

#[derive(Args, Debug)]
struct AddDependencyDprFilterArgs {
    /// Optional glob pattern for .dpr files to ignore (repeatable)
    #[arg(long, value_name = "GLOB", action = clap::ArgAction::Append)]
    ignore_dpr: Vec<String>,
}

#[derive(Args, Debug)]
#[command(group(
    ArgGroup::new("insert_targets")
        .required(true)
        .multiple(true)
        .args(["target_path", "target_dpr"])
))]
struct InsertDependencyTargetArgs {
    /// Directory whose .dpr files should be updated recursively (repeatable)
    #[arg(long, value_name = "PATH", action = clap::ArgAction::Append)]
    target_path: Vec<String>,

    /// Specific .dpr file to update (repeatable)
    #[arg(long, value_name = "DPR_FILE", action = clap::ArgAction::Append)]
    target_dpr: Vec<String>,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::AddDependency(args) => run_add_dependency(args),
        Commands::InsertDependency(args) => run_insert_dependency(args),
        Commands::DeleteDependency(args) => run_delete_dependency(args),
        Commands::FixDpr(args) => run_fix_dpr(args),
        Commands::ListConditionals(args) => run_list_conditionals(args),
    }
}

fn run_add_dependency(args: AddDependencyArgs) {
    let cwd = match env::current_dir() {
        Ok(path) => path,
        Err(err) => exit_with_error(format!("failed to read current directory: {err}"), 2),
    };
    let cwd = fs_walk::canonicalize_root(&cwd);

    let search_roots = match fs_walk::resolve_search_roots(&args.common.search_path, &cwd) {
        Ok(roots) => roots,
        Err(err) => exit_with_error(err, 2),
    };
    let mut delphi_roots =
        match fs_walk::resolve_optional_roots(&args.delphi_path, &cwd, "--delphi-path") {
            Ok(roots) => roots,
            Err(err) => exit_with_error(err, 2),
        };
    let mut delphi_roots_from_version = match delphi::resolve_source_roots(&args.delphi_version) {
        Ok(roots) => roots,
        Err(err) => exit_with_error(err, 2),
    };
    delphi_roots.append(&mut delphi_roots_from_version);
    delphi_roots = dedupe_paths(delphi_roots);

    let mut warnings = Vec::new();
    let dependency_assumptions =
        match build_dependency_assumptions(&args.dependency_lookup.assume_off) {
            Ok(value) => value,
            Err(err) => exit_with_error(err, 2),
        };
    let new_dependency_path = match resolve_new_dependency_path(&args.new_dependency, &cwd) {
        Ok(path) => path,
        Err(err) => exit_with_error(err, 2),
    };
    if let Err(err) = validate_new_dependency_path(&new_dependency_path) {
        exit_with_error(err, 2);
    }

    let ignore_matcher = match fs_walk::build_ignore_matcher(&args.common.ignore_path, &cwd) {
        Ok(matcher) => matcher,
        Err(err) => exit_with_error(err, 2),
    };
    let ignore_dpr_matcher =
        match fs_walk::build_dpr_ignore_matcher(&args.dpr_filter.ignore_dpr, &cwd) {
            Ok(matcher) => matcher,
            Err(err) => exit_with_error(err, 2),
        };

    println!("fixdpr {}", env!("CARGO_PKG_VERSION"));
    println!("Mode: add-dependency");
    println!("Scanning {} root(s):", search_roots.len());
    for root in &search_roots {
        println!("  {}", root.display());
    }
    if !delphi_roots.is_empty() {
        println!("Delphi fallback roots ({}):", delphi_roots.len());
        for root in &delphi_roots {
            println!("  {}", root.display());
        }
    }
    let delphi_version_display = format_values(&args.delphi_version);
    if !delphi_version_display.is_empty() {
        println!("Delphi version lookup: {}", delphi_version_display);
    }
    let ignore_display = format_values(&args.common.ignore_path);
    if !ignore_display.is_empty() {
        println!("Ignoring: {}", ignore_display);
    }
    let assume_off_display = format_values(&args.dependency_lookup.assume_off);
    if !assume_off_display.is_empty() {
        println!("Assuming off: {}", assume_off_display);
    }
    let ignore_dpr_display = format_values(ignore_dpr_matcher.normalized_patterns());
    if !ignore_dpr_display.is_empty() {
        println!("Ignoring dpr (absolute): {}", ignore_dpr_display);
    }

    let scan = match fs_walk::scan_files(&search_roots, &ignore_matcher) {
        Ok(result) => result,
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    let dpr_filter = fs_walk::filter_ignored_dpr_files(&scan.dpr_files, &ignore_dpr_matcher);
    let mut infos = Vec::new();
    for path in &dpr_filter.ignored_files {
        infos.push(format!("info: ignored dpr {}", path.display()));
    }

    println!(
        "Found {} .pas, {} .dpr",
        scan.pas_files.len(),
        scan.dpr_files.len()
    );
    println!("Building unit cache...");
    let mut unit_cache = match unit_cache::build_unit_cache(&scan.pas_files, &mut warnings) {
        Ok(result) => result,
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    println!("Unit cache ready ({} units)", scan.pas_files.len());

    let mut delphi_unit_cache = if delphi_roots.is_empty() {
        None
    } else {
        println!("Scanning Delphi fallback roots...");
        let delphi_scan =
            match fs_walk::scan_files(&delphi_roots, &fs_walk::IgnoreMatcher::default()) {
                Ok(result) => result,
                Err(err) => exit_with_error(err.to_string(), 1),
            };
        println!("Found {} fallback .pas", delphi_scan.pas_files.len());
        println!("Building Delphi fallback unit cache...");
        let cache = match unit_cache::build_unit_cache(&delphi_scan.pas_files, &mut warnings) {
            Ok(result) => result,
            Err(err) => exit_with_error(err.to_string(), 1),
        };
        println!(
            "Delphi fallback unit cache ready ({} units)",
            cache.by_path.len()
        );
        Some(cache)
    };

    let new_dependency_path = unit_cache::canonicalize_if_exists(&new_dependency_path);
    let new_unit = match unit_cache::load_unit_file(&new_dependency_path, &mut warnings) {
        Ok(Some(unit)) => unit,
        Ok(None) => {
            exit_with_error(
                format!(
                    "unable to determine unit name from new dependency: {}",
                    new_dependency_path.display()
                ),
                1,
            );
        }
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    println!(
        "New dependency: {} ({})",
        new_unit.name,
        new_unit.path.display()
    );

    println!("Updating .dpr files... {}", dpr_filter.included_files.len());
    let mut dpr_summary = match dpr_edit::update_dpr_files(
        &dpr_filter.included_files,
        &mut unit_cache,
        delphi_unit_cache.as_mut(),
        &new_unit,
        !args.disable_introduced_dependencies,
        &dependency_assumptions,
    ) {
        Ok(summary) => summary,
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    warnings.extend(dpr_summary.warnings.iter().cloned());

    if args.fix_updated_dprs && !dpr_summary.updated_paths.is_empty() {
        println!(
            "Running fix-dpr pass on updated dpr files... {}",
            dpr_summary.updated_paths.len()
        );
        let mut fix_pass_scanned = 0usize;
        let mut fix_pass_updated = 0usize;
        let mut fix_pass_failures = 0usize;
        let updated_paths = dpr_summary.updated_paths.clone();
        for dpr_path in &updated_paths {
            let fix_summary = match dpr_edit::fix_dpr_file(
                dpr_path,
                &unit_cache,
                delphi_unit_cache.as_ref(),
                &dependency_assumptions,
            ) {
                Ok(summary) => summary,
                Err(err) => {
                    warnings.push(format!(
                        "warning: failed to run fix-dpr on {}: {err}",
                        dpr_path.display()
                    ));
                    fix_pass_failures += 1;
                    continue;
                }
            };
            fix_pass_scanned += fix_summary.scanned;
            fix_pass_updated += fix_summary.updated;
            fix_pass_failures += fix_summary.failures;
            warnings.extend(fix_summary.warnings);
            for path in fix_summary.updated_paths {
                if !contains_path(&dpr_summary.updated_paths, &path) {
                    dpr_summary.updated_paths.push(path);
                }
            }
        }
        dpr_summary.updated = dpr_summary.updated_paths.len();
        dpr_summary.failures += fix_pass_failures;
        println!(
            "fix-dpr pass report: scanned {}, updated {}, failures {}",
            fix_pass_scanned, fix_pass_updated, fix_pass_failures
        );
    }

    print_summary(SummaryOutput {
        infos: &infos,
        warnings: &warnings,
        show_infos: args.common.show_infos,
        show_warnings: args.common.show_warnings,
        pas_scanned: scan.pas_files.len(),
        dpr_summary: &dpr_summary,
        ignored_dpr: dpr_filter.ignored_files.len(),
        search_roots: &search_roots,
    });

    if dpr_summary.failures > 0 {
        process::exit(1);
    }
}

fn run_fix_dpr(args: FixDprArgs) {
    let cwd = match env::current_dir() {
        Ok(path) => path,
        Err(err) => exit_with_error(format!("failed to read current directory: {err}"), 2),
    };
    let cwd = fs_walk::canonicalize_root(&cwd);

    let search_roots = match fs_walk::resolve_search_roots(&args.common.search_path, &cwd) {
        Ok(roots) => roots,
        Err(err) => exit_with_error(err, 2),
    };
    let mut delphi_roots =
        match fs_walk::resolve_optional_roots(&args.delphi_path, &cwd, "--delphi-path") {
            Ok(roots) => roots,
            Err(err) => exit_with_error(err, 2),
        };
    let mut delphi_roots_from_version = match delphi::resolve_source_roots(&args.delphi_version) {
        Ok(roots) => roots,
        Err(err) => exit_with_error(err, 2),
    };
    delphi_roots.append(&mut delphi_roots_from_version);
    delphi_roots = dedupe_paths(delphi_roots);
    let ignore_matcher = match fs_walk::build_ignore_matcher(&args.common.ignore_path, &cwd) {
        Ok(matcher) => matcher,
        Err(err) => exit_with_error(err, 2),
    };
    let target_dpr = match resolve_dpr_file_path(&args.dpr_file, &cwd) {
        Ok(path) => path,
        Err(err) => exit_with_error(err, 2),
    };
    if let Err(err) = validate_dpr_file_path(&target_dpr, "DPR_FILE") {
        exit_with_error(err, 2);
    }
    let target_dpr = unit_cache::canonicalize_if_exists(&target_dpr);
    let dependency_assumptions =
        match build_dependency_assumptions(&args.dependency_lookup.assume_off) {
            Ok(value) => value,
            Err(err) => exit_with_error(err, 2),
        };

    println!("fixdpr {}", env!("CARGO_PKG_VERSION"));
    println!("Mode: fix-dpr");
    println!("Target dpr: {}", target_dpr.display());
    println!("Scanning {} root(s):", search_roots.len());
    for root in &search_roots {
        println!("  {}", root.display());
    }
    if !delphi_roots.is_empty() {
        println!("Delphi fallback roots ({}):", delphi_roots.len());
        for root in &delphi_roots {
            println!("  {}", root.display());
        }
    }
    let delphi_version_display = format_values(&args.delphi_version);
    if !delphi_version_display.is_empty() {
        println!("Delphi version lookup: {}", delphi_version_display);
    }
    let ignore_display = format_values(&args.common.ignore_path);
    if !ignore_display.is_empty() {
        println!("Ignoring: {}", ignore_display);
    }
    let assume_off_display = format_values(&args.dependency_lookup.assume_off);
    if !assume_off_display.is_empty() {
        println!("Assuming off: {}", assume_off_display);
    }
    let scan = match fs_walk::scan_files(&search_roots, &ignore_matcher) {
        Ok(result) => result,
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    let infos = Vec::new();
    println!(
        "Found {} .pas, {} .dpr",
        scan.pas_files.len(),
        scan.dpr_files.len()
    );

    if !contains_path(&scan.dpr_files, &target_dpr) {
        exit_with_error(
            format!(
                "DPR_FILE not found under --search-path after ignore filters: {}",
                target_dpr.display()
            ),
            2,
        );
    }

    let mut warnings = Vec::new();
    println!("Building unit cache...");
    let unit_cache = match unit_cache::build_unit_cache(&scan.pas_files, &mut warnings) {
        Ok(result) => result,
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    println!("Unit cache ready ({} units)", scan.pas_files.len());
    let delphi_unit_cache = if delphi_roots.is_empty() {
        None
    } else {
        println!("Scanning Delphi fallback roots...");
        let delphi_scan =
            match fs_walk::scan_files(&delphi_roots, &fs_walk::IgnoreMatcher::default()) {
                Ok(result) => result,
                Err(err) => exit_with_error(err.to_string(), 1),
            };
        println!("Found {} fallback .pas", delphi_scan.pas_files.len());
        println!("Building Delphi fallback unit cache...");
        let cache = match unit_cache::build_unit_cache(&delphi_scan.pas_files, &mut warnings) {
            Ok(result) => result,
            Err(err) => exit_with_error(err.to_string(), 1),
        };
        println!(
            "Delphi fallback unit cache ready ({} units)",
            cache.by_path.len()
        );
        Some(cache)
    };
    println!("Repairing target dpr...");

    let dpr_summary = match dpr_edit::fix_dpr_file(
        &target_dpr,
        &unit_cache,
        delphi_unit_cache.as_ref(),
        &dependency_assumptions,
    ) {
        Ok(summary) => summary,
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    warnings.extend(dpr_summary.warnings.iter().cloned());

    print_summary(SummaryOutput {
        infos: &infos,
        warnings: &warnings,
        show_infos: args.common.show_infos,
        show_warnings: args.common.show_warnings,
        pas_scanned: scan.pas_files.len(),
        dpr_summary: &dpr_summary,
        ignored_dpr: 0,
        search_roots: &search_roots,
    });

    if dpr_summary.failures > 0 {
        process::exit(1);
    }
}

fn run_list_conditionals(args: ListConditionalsArgs) {
    let cwd = match env::current_dir() {
        Ok(path) => path,
        Err(err) => exit_with_error(format!("failed to read current directory: {err}"), 2),
    };
    let cwd = fs_walk::canonicalize_root(&cwd);

    let search_roots = match fs_walk::resolve_search_roots(&args.common.search_path, &cwd) {
        Ok(roots) => roots,
        Err(err) => exit_with_error(err, 2),
    };
    let mut delphi_roots =
        match fs_walk::resolve_optional_roots(&args.delphi_path, &cwd, "--delphi-path") {
            Ok(roots) => roots,
            Err(err) => exit_with_error(err, 2),
        };
    let mut delphi_roots_from_version = match delphi::resolve_source_roots(&args.delphi_version) {
        Ok(roots) => roots,
        Err(err) => exit_with_error(err, 2),
    };
    delphi_roots.append(&mut delphi_roots_from_version);
    delphi_roots = dedupe_paths(delphi_roots);
    let ignore_matcher = match fs_walk::build_ignore_matcher(&args.common.ignore_path, &cwd) {
        Ok(matcher) => matcher,
        Err(err) => exit_with_error(err, 2),
    };
    let target_dpr = match resolve_dpr_file_path(&args.dpr_file, &cwd) {
        Ok(path) => path,
        Err(err) => exit_with_error(err, 2),
    };
    if let Err(err) = validate_dpr_file_path(&target_dpr, "DPR_FILE") {
        exit_with_error(err, 2);
    }
    let target_dpr = unit_cache::canonicalize_if_exists(&target_dpr);
    let dependency_assumptions =
        match build_dependency_assumptions(&args.dependency_lookup.assume_off) {
            Ok(value) => value,
            Err(err) => exit_with_error(err, 2),
        };

    println!("fixdpr {}", env!("CARGO_PKG_VERSION"));
    println!("Mode: list-conditionals");
    println!("Target dpr: {}", target_dpr.display());
    println!("Scanning {} root(s):", search_roots.len());
    for root in &search_roots {
        println!("  {}", root.display());
    }
    if !delphi_roots.is_empty() {
        println!("Delphi fallback roots ({}):", delphi_roots.len());
        for root in &delphi_roots {
            println!("  {}", root.display());
        }
    }
    let delphi_version_display = format_values(&args.delphi_version);
    if !delphi_version_display.is_empty() {
        println!("Delphi version lookup: {}", delphi_version_display);
    }
    let ignore_display = format_values(&args.common.ignore_path);
    if !ignore_display.is_empty() {
        println!("Ignoring: {}", ignore_display);
    }
    let assume_off_display = format_values(&args.dependency_lookup.assume_off);
    if !assume_off_display.is_empty() {
        println!("Assuming off: {}", assume_off_display);
    }

    let scan = match fs_walk::scan_files(&search_roots, &ignore_matcher) {
        Ok(result) => result,
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    println!(
        "Found {} .pas, {} .dpr",
        scan.pas_files.len(),
        scan.dpr_files.len()
    );

    if !contains_path(&scan.dpr_files, &target_dpr) {
        exit_with_error(
            format!(
                "DPR_FILE not found under --search-path after ignore filters: {}",
                target_dpr.display()
            ),
            2,
        );
    }

    let mut warnings = Vec::new();
    println!("Building unit cache...");
    let unit_cache = match unit_cache::build_unit_cache(&scan.pas_files, &mut warnings) {
        Ok(result) => result,
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    println!("Unit cache ready ({} units)", scan.pas_files.len());
    let delphi_unit_cache = if delphi_roots.is_empty() {
        None
    } else {
        println!("Scanning Delphi fallback roots...");
        let delphi_scan =
            match fs_walk::scan_files(&delphi_roots, &fs_walk::IgnoreMatcher::default()) {
                Ok(result) => result,
                Err(err) => exit_with_error(err.to_string(), 1),
            };
        println!("Found {} fallback .pas", delphi_scan.pas_files.len());
        println!("Building Delphi fallback unit cache...");
        let cache = match unit_cache::build_unit_cache(&delphi_scan.pas_files, &mut warnings) {
            Ok(result) => result,
            Err(err) => exit_with_error(err.to_string(), 1),
        };
        println!(
            "Delphi fallback unit cache ready ({} units)",
            cache.by_path.len()
        );
        Some(cache)
    };

    println!("Analyzing target dpr conditionals...");
    let conditional_units = match conditionals::collect_dpr_conditional_units(
        &target_dpr,
        &unit_cache,
        delphi_unit_cache.as_ref(),
        &dependency_assumptions,
        &mut warnings,
    ) {
        Ok(Some(units)) => units,
        Ok(None) => exit_with_error(format!("no uses list found in {}", target_dpr.display()), 1),
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    let buckets = conditionals::bucket_conditionals(&conditional_units);

    print_conditionals_summary(ConditionalsOutput {
        warnings: &warnings,
        show_warnings: args.common.show_warnings,
        pas_scanned: scan.pas_files.len(),
        dpr_scanned: 1,
        buckets: &buckets,
    });
}

fn run_insert_dependency(args: InsertDependencyArgs) {
    let cwd = match env::current_dir() {
        Ok(path) => path,
        Err(err) => exit_with_error(format!("failed to read current directory: {err}"), 2),
    };
    let cwd = fs_walk::canonicalize_root(&cwd);

    let search_roots = match fs_walk::resolve_search_roots(&args.common.search_path, &cwd) {
        Ok(roots) => roots,
        Err(err) => exit_with_error(err, 2),
    };
    let target_paths =
        match fs_walk::resolve_optional_roots(&args.targets.target_path, &cwd, "--target-path") {
            Ok(paths) => paths,
            Err(err) => exit_with_error(err, 2),
        };
    if let Err(err) = ensure_paths_under_search_roots(&target_paths, &search_roots, "--target-path")
    {
        exit_with_error(err, 2);
    }

    let target_dprs = match resolve_target_dpr_paths(&args.targets.target_dpr, &cwd) {
        Ok(paths) => paths,
        Err(err) => exit_with_error(err, 2),
    };
    if let Err(err) = ensure_paths_under_search_roots(&target_dprs, &search_roots, "--target-dpr") {
        exit_with_error(err, 2);
    }

    let mut delphi_roots =
        match fs_walk::resolve_optional_roots(&args.delphi_path, &cwd, "--delphi-path") {
            Ok(roots) => roots,
            Err(err) => exit_with_error(err, 2),
        };
    let mut delphi_roots_from_version = match delphi::resolve_source_roots(&args.delphi_version) {
        Ok(roots) => roots,
        Err(err) => exit_with_error(err, 2),
    };
    delphi_roots.append(&mut delphi_roots_from_version);
    delphi_roots = dedupe_paths(delphi_roots);

    let mut warnings = Vec::new();
    let dependency_assumptions =
        match build_dependency_assumptions(&args.dependency_lookup.assume_off) {
            Ok(value) => value,
            Err(err) => exit_with_error(err, 2),
        };
    let new_dependency_path = match resolve_new_dependency_path(&args.new_dependency, &cwd) {
        Ok(path) => path,
        Err(err) => exit_with_error(err, 2),
    };
    if let Err(err) = validate_new_dependency_path(&new_dependency_path) {
        exit_with_error(err, 2);
    }

    let ignore_matcher = match fs_walk::build_ignore_matcher(&args.common.ignore_path, &cwd) {
        Ok(matcher) => matcher,
        Err(err) => exit_with_error(err, 2),
    };
    let ignore_dpr_matcher =
        match fs_walk::build_dpr_ignore_matcher(&args.dpr_filter.ignore_dpr, &cwd) {
            Ok(matcher) => matcher,
            Err(err) => exit_with_error(err, 2),
        };

    println!("fixdpr {}", env!("CARGO_PKG_VERSION"));
    println!("Mode: insert-dependency");
    println!("Scanning {} root(s):", search_roots.len());
    for root in &search_roots {
        println!("  {}", root.display());
    }
    if !target_paths.is_empty() {
        println!("Target paths ({}):", target_paths.len());
        for path in &target_paths {
            println!("  {}", path.display());
        }
    }
    if !target_dprs.is_empty() {
        println!("Target dpr files ({}):", target_dprs.len());
        for path in &target_dprs {
            println!("  {}", path.display());
        }
    }
    if !delphi_roots.is_empty() {
        println!("Delphi fallback roots ({}):", delphi_roots.len());
        for root in &delphi_roots {
            println!("  {}", root.display());
        }
    }
    let delphi_version_display = format_values(&args.delphi_version);
    if !delphi_version_display.is_empty() {
        println!("Delphi version lookup: {}", delphi_version_display);
    }
    let ignore_display = format_values(&args.common.ignore_path);
    if !ignore_display.is_empty() {
        println!("Ignoring: {}", ignore_display);
    }
    let assume_off_display = format_values(&args.dependency_lookup.assume_off);
    if !assume_off_display.is_empty() {
        println!("Assuming off: {}", assume_off_display);
    }
    let ignore_dpr_display = format_values(ignore_dpr_matcher.normalized_patterns());
    if !ignore_dpr_display.is_empty() {
        println!("Ignoring dpr (absolute): {}", ignore_dpr_display);
    }

    let scan = match fs_walk::scan_files(&search_roots, &ignore_matcher) {
        Ok(result) => result,
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    let (target_dpr_files, ignored_target_dprs) = match select_target_dpr_files(
        &scan.dpr_files,
        &target_paths,
        &target_dprs,
        &ignore_dpr_matcher,
    ) {
        Ok(value) => value,
        Err(err) => exit_with_error(err, 2),
    };
    let mut infos = Vec::new();
    for path in &ignored_target_dprs {
        infos.push(format!("info: ignored dpr {}", path.display()));
    }

    println!(
        "Found {} .pas, {} .dpr",
        scan.pas_files.len(),
        scan.dpr_files.len()
    );
    println!("Updating selected .dpr files... {}", target_dpr_files.len());
    println!("Building unit cache...");
    let mut unit_cache = match unit_cache::build_unit_cache(&scan.pas_files, &mut warnings) {
        Ok(result) => result,
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    println!("Unit cache ready ({} units)", scan.pas_files.len());

    let mut delphi_unit_cache = if delphi_roots.is_empty() {
        None
    } else {
        println!("Scanning Delphi fallback roots...");
        let delphi_scan =
            match fs_walk::scan_files(&delphi_roots, &fs_walk::IgnoreMatcher::default()) {
                Ok(result) => result,
                Err(err) => exit_with_error(err.to_string(), 1),
            };
        println!("Found {} fallback .pas", delphi_scan.pas_files.len());
        println!("Building Delphi fallback unit cache...");
        let cache = match unit_cache::build_unit_cache(&delphi_scan.pas_files, &mut warnings) {
            Ok(result) => result,
            Err(err) => exit_with_error(err.to_string(), 1),
        };
        println!(
            "Delphi fallback unit cache ready ({} units)",
            cache.by_path.len()
        );
        Some(cache)
    };

    let new_dependency_path = unit_cache::canonicalize_if_exists(&new_dependency_path);
    let new_unit = match unit_cache::load_unit_file(&new_dependency_path, &mut warnings) {
        Ok(Some(unit)) => unit,
        Ok(None) => {
            exit_with_error(
                format!(
                    "unable to determine unit name from new dependency: {}",
                    new_dependency_path.display()
                ),
                1,
            );
        }
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    println!(
        "New dependency: {} ({})",
        new_unit.name,
        new_unit.path.display()
    );

    let dpr_summary = match dpr_edit::insert_dependency_files(
        &target_dpr_files,
        &mut unit_cache,
        delphi_unit_cache.as_mut(),
        &new_unit,
        !args.disable_introduced_dependencies,
        &dependency_assumptions,
    ) {
        Ok(summary) => summary,
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    warnings.extend(dpr_summary.warnings.iter().cloned());

    print_summary(SummaryOutput {
        infos: &infos,
        warnings: &warnings,
        show_infos: args.common.show_infos,
        show_warnings: args.common.show_warnings,
        pas_scanned: scan.pas_files.len(),
        dpr_summary: &dpr_summary,
        ignored_dpr: ignored_target_dprs.len(),
        search_roots: &search_roots,
    });

    if dpr_summary.failures > 0 {
        process::exit(1);
    }
}

fn run_delete_dependency(args: DeleteDependencyArgs) {
    let cwd = match env::current_dir() {
        Ok(path) => path,
        Err(err) => exit_with_error(format!("failed to read current directory: {err}"), 2),
    };
    let cwd = fs_walk::canonicalize_root(&cwd);

    let search_roots = match fs_walk::resolve_search_roots(&args.common.search_path, &cwd) {
        Ok(roots) => roots,
        Err(err) => exit_with_error(err, 2),
    };
    let target_paths =
        match fs_walk::resolve_optional_roots(&args.targets.target_path, &cwd, "--target-path") {
            Ok(paths) => paths,
            Err(err) => exit_with_error(err, 2),
        };
    if let Err(err) = ensure_paths_under_search_roots(&target_paths, &search_roots, "--target-path")
    {
        exit_with_error(err, 2);
    }

    let target_dprs = match resolve_target_dpr_paths(&args.targets.target_dpr, &cwd) {
        Ok(paths) => paths,
        Err(err) => exit_with_error(err, 2),
    };
    if let Err(err) = ensure_paths_under_search_roots(&target_dprs, &search_roots, "--target-dpr") {
        exit_with_error(err, 2);
    }

    let mut delphi_roots =
        match fs_walk::resolve_optional_roots(&args.delphi_path, &cwd, "--delphi-path") {
            Ok(roots) => roots,
            Err(err) => exit_with_error(err, 2),
        };
    let mut delphi_roots_from_version = match delphi::resolve_source_roots(&args.delphi_version) {
        Ok(roots) => roots,
        Err(err) => exit_with_error(err, 2),
    };
    delphi_roots.append(&mut delphi_roots_from_version);
    delphi_roots = dedupe_paths(delphi_roots);

    let dependency_assumptions =
        match build_dependency_assumptions(&args.dependency_lookup.assume_off) {
            Ok(value) => value,
            Err(err) => exit_with_error(err, 2),
        };
    let old_dependency_path = match resolve_new_dependency_path(&args.old_dependency, &cwd) {
        Ok(path) => path,
        Err(err) => exit_with_error(err, 2),
    };
    if let Err(err) = validate_new_dependency_path(&old_dependency_path) {
        exit_with_error(err, 2);
    }

    let ignore_matcher = match fs_walk::build_ignore_matcher(&args.common.ignore_path, &cwd) {
        Ok(matcher) => matcher,
        Err(err) => exit_with_error(err, 2),
    };
    let ignore_dpr_matcher =
        match fs_walk::build_dpr_ignore_matcher(&args.dpr_filter.ignore_dpr, &cwd) {
            Ok(matcher) => matcher,
            Err(err) => exit_with_error(err, 2),
        };

    let mut warnings = Vec::new();
    println!("fixdpr {}", env!("CARGO_PKG_VERSION"));
    println!("Mode: delete-dependency");
    println!("Scanning {} root(s):", search_roots.len());
    for root in &search_roots {
        println!("  {}", root.display());
    }
    if !delphi_roots.is_empty() {
        println!("Delphi fallback roots ({}):", delphi_roots.len());
        for root in &delphi_roots {
            println!("  {}", root.display());
        }
    }
    let delphi_version_display = format_values(&args.delphi_version);
    if !delphi_version_display.is_empty() {
        println!("Delphi version lookup: {}", delphi_version_display);
    }
    let ignore_display = format_values(&args.common.ignore_path);
    if !ignore_display.is_empty() {
        println!("Ignoring: {}", ignore_display);
    }
    let assume_off_display = format_values(&args.dependency_lookup.assume_off);
    if !assume_off_display.is_empty() {
        println!("Assuming off: {}", assume_off_display);
    }
    let ignore_dpr_display = format_values(ignore_dpr_matcher.normalized_patterns());
    if !ignore_dpr_display.is_empty() {
        println!("Ignoring dpr (absolute): {}", ignore_dpr_display);
    }

    let scan = match fs_walk::scan_files(&search_roots, &ignore_matcher) {
        Ok(result) => result,
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    let (target_dpr_files, ignored_target_dprs) = match select_target_dpr_files(
        &scan.dpr_files,
        &target_paths,
        &target_dprs,
        &ignore_dpr_matcher,
    ) {
        Ok(value) => value,
        Err(err) => exit_with_error(err, 2),
    };
    let mut infos = Vec::new();
    for path in &ignored_target_dprs {
        infos.push(format!("info: ignored dpr {}", path.display()));
    }

    println!(
        "Found {} .pas, {} .dpr",
        scan.pas_files.len(),
        scan.dpr_files.len()
    );
    println!("Updating selected .dpr files... {}", target_dpr_files.len());
    println!("Building unit cache...");
    let unit_cache = match unit_cache::build_unit_cache(&scan.pas_files, &mut warnings) {
        Ok(result) => result,
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    println!("Unit cache ready ({} units)", scan.pas_files.len());

    let delphi_unit_cache = if delphi_roots.is_empty() {
        None
    } else {
        println!("Scanning Delphi fallback roots...");
        let delphi_scan =
            match fs_walk::scan_files(&delphi_roots, &fs_walk::IgnoreMatcher::default()) {
                Ok(result) => result,
                Err(err) => exit_with_error(err.to_string(), 1),
            };
        println!("Found {} fallback .pas", delphi_scan.pas_files.len());
        println!("Building Delphi fallback unit cache...");
        let cache = match unit_cache::build_unit_cache(&delphi_scan.pas_files, &mut warnings) {
            Ok(result) => result,
            Err(err) => exit_with_error(err.to_string(), 1),
        };
        println!(
            "Delphi fallback unit cache ready ({} units)",
            cache.by_path.len()
        );
        Some(cache)
    };

    let old_dependency_path = unit_cache::canonicalize_if_exists(&old_dependency_path);
    let old_unit = match unit_cache::load_unit_file(&old_dependency_path, &mut warnings) {
        Ok(Some(unit)) => unit,
        Ok(None) => {
            exit_with_error(
                format!(
                    "unable to determine unit name from old dependency: {}",
                    old_dependency_path.display()
                ),
                1,
            );
        }
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    println!(
        "Old dependency: {} ({})",
        old_unit.name,
        old_unit.path.display()
    );

    let dpr_summary = match dpr_edit::delete_dependency_files(
        &target_dpr_files,
        &unit_cache,
        delphi_unit_cache.as_ref(),
        &old_unit.name,
        &dependency_assumptions,
    ) {
        Ok(summary) => summary,
        Err(err) => exit_with_error(err.to_string(), 1),
    };
    warnings.extend(dpr_summary.warnings.iter().cloned());

    print_summary(SummaryOutput {
        infos: &infos,
        warnings: &warnings,
        show_infos: args.common.show_infos,
        show_warnings: args.common.show_warnings,
        pas_scanned: scan.pas_files.len(),
        dpr_summary: &dpr_summary,
        ignored_dpr: ignored_target_dprs.len(),
        search_roots: &search_roots,
    });

    if dpr_summary.failures > 0 {
        process::exit(1);
    }
}

struct SummaryOutput<'a> {
    infos: &'a [String],
    warnings: &'a [String],
    show_infos: bool,
    show_warnings: bool,
    pas_scanned: usize,
    dpr_summary: &'a dpr_edit::DprUpdateSummary,
    ignored_dpr: usize,
    search_roots: &'a [PathBuf],
}

struct ConditionalsOutput<'a> {
    warnings: &'a [String],
    show_warnings: bool,
    pas_scanned: usize,
    dpr_scanned: usize,
    buckets: &'a conditionals::ConditionBuckets,
}

fn print_summary(summary: SummaryOutput<'_>) {
    let SummaryOutput {
        infos,
        warnings,
        show_infos,
        show_warnings,
        pas_scanned,
        dpr_summary,
        ignored_dpr,
        search_roots,
    } = summary;

    let unchanged = dpr_summary
        .scanned
        .saturating_sub(dpr_summary.updated)
        .saturating_sub(dpr_summary.failures);

    println!();
    println!("Infos: {}", infos.len());
    if show_infos && !infos.is_empty() {
        println!("Infos list:");
        for info in infos {
            println!("  {info}");
        }
    }
    println!("Warnings: {}", warnings.len());
    if show_warnings && !warnings.is_empty() {
        println!("Warnings list:");
        for warning in warnings {
            println!("  {warning}");
        }
    }
    println!();
    println!("Report:");
    println!("  pas scanned: {}", pas_scanned);
    println!("  dpr scanned: {}", dpr_summary.scanned);
    println!("  dpr ignored: {}", ignored_dpr);
    println!("  dpr updated: {}", dpr_summary.updated);
    println!("  dpr unchanged: {}", unchanged);
    println!("  dpr failures: {}", dpr_summary.failures);
    println!("Updated dpr files ({}):", dpr_summary.updated);
    if dpr_summary.updated_paths.is_empty() {
        println!("  (none)");
    } else {
        for path in &dpr_summary.updated_paths {
            println!("  {}", display_path(path, search_roots));
        }
    }
}

fn print_conditionals_summary(summary: ConditionalsOutput<'_>) {
    let ConditionalsOutput {
        warnings,
        show_warnings,
        pas_scanned,
        dpr_scanned,
        buckets,
    } = summary;

    println!();
    println!("Warnings: {}", warnings.len());
    if show_warnings && !warnings.is_empty() {
        println!("Warnings list:");
        for warning in warnings {
            println!("  {warning}");
        }
    }

    println!();
    println!("Report:");
    println!("  pas scanned: {}", pas_scanned);
    println!("  dpr scanned: {}", dpr_scanned);
    println!();
    println!("Unconditional units ({}):", buckets.unconditional.len());
    if buckets.unconditional.is_empty() {
        println!("  (none)");
    } else {
        for unit in &buckets.unconditional {
            println!("  {unit}");
        }
    }

    if buckets.positive.is_empty() {
        println!("Units only if X is defined: (none)");
    } else {
        for (symbol, units) in &buckets.positive {
            println!("Units only if {} is defined ({}):", symbol, units.len());
            for unit in units {
                println!("  {unit}");
            }
        }
    }

    if buckets.negative.is_empty() {
        println!("Units only if Y is not defined: (none)");
    } else {
        for (symbol, units) in &buckets.negative {
            println!("Units only if {} is not defined ({}):", symbol, units.len());
            for unit in units {
                println!("  {unit}");
            }
        }
    }

    println!("Units with complex conditions ({}):", buckets.complex.len());
    if buckets.complex.is_empty() {
        println!("  (none)");
    } else {
        for (unit, condition) in &buckets.complex {
            println!("  {}: {}", unit, condition);
        }
    }
}

fn resolve_new_dependency_path(value: &str, cwd: &Path) -> Result<PathBuf, String> {
    resolve_path_with_flag(value, cwd, "NEW_DEPENDENCY")
}

fn resolve_dpr_file_path(value: &str, cwd: &Path) -> Result<PathBuf, String> {
    resolve_path_with_flag(value, cwd, "DPR_FILE")
}

fn resolve_target_dpr_paths(values: &[String], cwd: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for value in values {
        let path = resolve_path_with_flag(value, cwd, "--target-dpr")?;
        validate_dpr_file_path(&path, "--target-dpr")?;
        paths.push(unit_cache::canonicalize_if_exists(&path));
    }
    Ok(dedupe_paths(paths))
}

fn resolve_path_with_flag(value: &str, cwd: &Path, flag_name: &str) -> Result<PathBuf, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{flag_name} cannot be empty"));
    }

    let mut path = PathBuf::from(trimmed);
    if path.is_relative() {
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

fn is_dpr_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("dpr"))
        .unwrap_or(false)
}

fn validate_new_dependency_path(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("NEW_DEPENDENCY path not found: {}", path.display()));
    }
    if !is_pas_file(path) {
        return Err(format!(
            "NEW_DEPENDENCY must point to a .pas file: {}",
            path.display()
        ));
    }
    Ok(())
}

fn validate_dpr_file_path(path: &Path, flag_name: &str) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("{flag_name} path not found: {}", path.display()));
    }
    if !is_dpr_file(path) {
        return Err(format!(
            "{flag_name} must point to a .dpr file: {}",
            path.display()
        ));
    }
    Ok(())
}

fn format_values(values: &[String]) -> String {
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

fn build_dependency_assumptions(
    assume_off: &[String],
) -> Result<conditionals::Assumptions, String> {
    let mut assumptions = conditionals::Assumptions::default();
    for symbol in assume_off {
        let trimmed = symbol.trim();
        if trimmed.is_empty() {
            return Err("--assume-off cannot be empty".to_string());
        }
        assumptions.set(trimmed, conditionals::AssumedValue::Off);
    }
    Ok(assumptions)
}

fn contains_path(paths: &[PathBuf], target: &Path) -> bool {
    let target_key = normalize_path_key(target);
    paths
        .iter()
        .any(|path| normalize_path_key(path) == target_key)
}

fn ensure_paths_under_search_roots(
    paths: &[PathBuf],
    search_roots: &[PathBuf],
    flag_name: &str,
) -> Result<(), String> {
    for path in paths {
        if search_roots.iter().any(|root| path.starts_with(root)) {
            continue;
        }
        return Err(format!(
            "{flag_name} must be under --search-path: {}",
            path.display()
        ));
    }
    Ok(())
}

fn select_target_dpr_files(
    scanned_dprs: &[PathBuf],
    target_paths: &[PathBuf],
    target_dprs: &[PathBuf],
    ignore_dpr_matcher: &fs_walk::DprIgnoreMatcher,
) -> Result<(Vec<PathBuf>, Vec<PathBuf>), String> {
    let mut selected = Vec::new();
    let mut ignored = Vec::new();

    for dpr_path in scanned_dprs {
        let selected_by_path = target_paths
            .iter()
            .any(|target| dpr_path.starts_with(target));
        let selected_by_file = contains_path(target_dprs, dpr_path);
        if !selected_by_path && !selected_by_file {
            continue;
        }

        if ignore_dpr_matcher.is_ignored(&dpr_path.to_string_lossy()) {
            ignored.push(dpr_path.clone());
        } else {
            selected.push(dpr_path.clone());
        }
    }

    for target_dpr in target_dprs {
        if contains_path(scanned_dprs, target_dpr) {
            continue;
        }
        return Err(format!(
            "--target-dpr not found under --search-path after ignore filters: {}",
            target_dpr.display()
        ));
    }

    Ok((selected, ignored))
}

fn normalize_path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn display_path(path: &Path, roots: &[PathBuf]) -> String {
    for root in roots {
        if path.starts_with(root) {
            return diff_paths(path, root)
                .unwrap_or_else(|| path.to_path_buf())
                .to_string_lossy()
                .to_string();
        }
    }

    path.to_string_lossy().to_string()
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();
    let mut seen = HashSet::new();

    for path in paths {
        let key = normalize_path_key(&path);
        if seen.insert(key) {
            deduped.push(path);
        }
    }

    deduped.sort_by_key(|path| normalize_path_key(path));
    deduped
}

fn exit_with_error(message: impl AsRef<str>, code: i32) -> ! {
    eprintln!("error: {}", message.as_ref());
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;

    #[test]
    fn parse_add_dependency_with_positional_new_dependency() {
        let parsed = Cli::try_parse_from([
            "fixdpr",
            "add-dependency",
            "--search-path",
            ".",
            "./common/NewUnit.pas",
        ]);

        assert!(parsed.is_ok(), "{parsed:?}");
    }

    #[test]
    fn reject_legacy_new_dependency_flag() {
        let parsed = Cli::try_parse_from([
            "fixdpr",
            "add-dependency",
            "--search-path",
            ".",
            "--new-dependency",
            "./common/NewUnit.pas",
        ]);

        assert!(parsed.is_err(), "legacy flag should not parse");
    }

    #[test]
    fn parse_add_dependency_with_assume_off() {
        let parsed = Cli::try_parse_from([
            "fixdpr",
            "add-dependency",
            "--search-path",
            ".",
            "--assume-off",
            "DEBUG",
            "./common/NewUnit.pas",
        ]);

        assert!(parsed.is_ok(), "{parsed:?}");
    }

    #[test]
    fn parse_insert_dependency_with_target_path_and_target_dpr() {
        let parsed = Cli::try_parse_from([
            "fixdpr",
            "insert-dependency",
            "--search-path",
            ".",
            "--target-path",
            "./apps",
            "--target-dpr",
            "./apps/App1.dpr",
            "./common/NewUnit.pas",
        ]);

        assert!(parsed.is_ok(), "{parsed:?}");
    }

    #[test]
    fn reject_insert_dependency_without_targets() {
        let parsed = Cli::try_parse_from([
            "fixdpr",
            "insert-dependency",
            "--search-path",
            ".",
            "./common/NewUnit.pas",
        ]);

        assert!(parsed.is_err(), "insert-dependency should require a target");
    }

    #[test]
    fn parse_fix_dpr_with_positional_dpr_file() {
        let parsed =
            Cli::try_parse_from(["fixdpr", "fix-dpr", "--search-path", ".", "./app1/App1.dpr"]);

        assert!(parsed.is_ok(), "{parsed:?}");
    }

    #[test]
    fn parse_fix_dpr_with_assume_off() {
        let parsed = Cli::try_parse_from([
            "fixdpr",
            "fix-dpr",
            "--search-path",
            ".",
            "--assume-off",
            "DEBUG",
            "./app1/App1.dpr",
        ]);

        assert!(parsed.is_ok(), "{parsed:?}");
    }

    #[test]
    fn reject_ignore_dpr_in_fix_dpr_mode() {
        let parsed = Cli::try_parse_from([
            "fixdpr",
            "fix-dpr",
            "--search-path",
            ".",
            "./app1/App1.dpr",
            "--ignore-dpr",
            "./app1/App1.dpr",
        ]);

        assert!(
            parsed.is_err(),
            "--ignore-dpr should not parse in fix-dpr mode"
        );
    }

    #[test]
    fn parse_list_conditionals_with_positional_dpr_file() {
        let parsed = Cli::try_parse_from([
            "fixdpr",
            "list-conditionals",
            "--search-path",
            ".",
            "./app1/App1.dpr",
        ]);

        assert!(parsed.is_ok(), "{parsed:?}");
    }

    #[test]
    fn reject_ignore_dpr_in_list_conditionals_mode() {
        let parsed = Cli::try_parse_from([
            "fixdpr",
            "list-conditionals",
            "--search-path",
            ".",
            "./app1/App1.dpr",
            "--ignore-dpr",
            "./app1/App1.dpr",
        ]);

        assert!(
            parsed.is_err(),
            "--ignore-dpr should not parse in list-conditionals mode"
        );
    }

    #[test]
    fn parse_delete_dependency_with_target_path() {
        let parsed = Cli::try_parse_from([
            "fixdpr",
            "delete-dependency",
            "--search-path",
            ".",
            "--target-path",
            "./apps",
            "./common/LegacyUnit.pas",
        ]);

        assert!(parsed.is_ok(), "{parsed:?}");
    }
}
