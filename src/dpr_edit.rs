use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::pas_lex;
use crate::unit_cache::{self, UnitCache, UnitFileInfo};
use crate::uses_include;

#[derive(Debug)]
pub struct DprUpdateSummary {
    pub scanned: usize,
    pub updated: usize,
    pub updated_paths: Vec<PathBuf>,
    pub warnings: Vec<String>,
    pub failures: usize,
}

#[derive(Debug)]
struct UsesEntry {
    name: String,
    in_path: Option<String>,
    start: usize,
    delimiter: Option<u8>,
    delimiter_pos: Option<usize>,
    from_include: bool,
}

#[derive(Debug)]
struct UsesList {
    entries: Vec<UsesEntry>,
    semicolon: usize,
    multiline: bool,
    indent: String,
    has_backslash: bool,
    has_slash: bool,
}

pub fn update_dpr_files(
    dpr_paths: &[PathBuf],
    project_cache: &mut UnitCache,
    mut delphi_cache: Option<&mut UnitCache>,
    new_unit: &UnitFileInfo,
    add_introduced_dependencies: bool,
) -> io::Result<DprUpdateSummary> {
    let mut summary = DprUpdateSummary {
        scanned: 0,
        updated: 0,
        updated_paths: Vec::new(),
        warnings: Vec::new(),
        failures: 0,
    };

    'dpr_loop: for path in dpr_paths {
        summary.scanned += 1;
        let bytes = match fs::read(path) {
            Ok(data) => data,
            Err(err) => {
                summary.warnings.push(format!(
                    "warning: failed to read dpr {}: {err}",
                    path.display()
                ));
                summary.failures += 1;
                continue;
            }
        };
        let Some(list) = parse_dpr_uses(path, &bytes, &mut summary.warnings) else {
            summary
                .warnings
                .push(format!("warning: no uses list found in {}", path.display()));
            summary.failures += 1;
            continue;
        };
        let mut current_bytes = bytes;
        let mut current_list = list;

        let project_map = build_project_map(
            path,
            &current_list,
            project_cache,
            delphi_cache.as_deref(),
            &mut summary.warnings,
        );
        let has_new_unit = current_list
            .entries
            .iter()
            .any(|entry| entry.name.eq_ignore_ascii_case(&new_unit.name));

        let mut needs_new_unit = false;
        let mut insert_after = None;
        if !has_new_unit {
            if project_map.is_empty() {
                continue;
            }

            let dependents = compute_project_dependents(
                project_cache,
                delphi_cache.as_deref_mut(),
                &project_map,
                new_unit,
                &mut summary.warnings,
            )?;

            for entry in &current_list.entries {
                let key = entry.name.to_ascii_lowercase();
                if let Some(path) = project_map.get(&key) {
                    if let Some(&id) = dependents.id_by_path.get(path) {
                        if dependents.dependents[id] {
                            needs_new_unit = true;
                            break;
                        }
                    }
                }
            }

            if !needs_new_unit {
                continue;
            }
            insert_after = find_direct_introducer_index(&current_list, &project_map, &dependents);
        }

        let mut dpr_updated = false;
        let mut last_inserted_name = None;

        if needs_new_unit {
            let updated = match insert_new_unit(
                &current_bytes,
                path,
                &current_list,
                new_unit,
                insert_after,
            ) {
                Ok(value) => value,
                Err(err) => {
                    summary.warnings.push(format!(
                        "warning: failed to update dpr {}: {err}",
                        path.display()
                    ));
                    summary.failures += 1;
                    continue;
                }
            };
            if updated {
                dpr_updated = true;
                last_inserted_name = Some(new_unit.name.clone());
                let reloaded = match reload_dpr_state(path, &mut summary.warnings) {
                    Ok(Some(value)) => value,
                    Ok(None) => {
                        summary
                            .warnings
                            .push(format!("warning: no uses list found in {}", path.display()));
                        summary.failures += 1;
                        continue 'dpr_loop;
                    }
                    Err(err) => {
                        summary.warnings.push(format!(
                            "warning: failed to read dpr {}: {err}",
                            path.display()
                        ));
                        summary.failures += 1;
                        continue 'dpr_loop;
                    }
                };
                current_bytes = reloaded.0;
                current_list = reloaded.1;
            }
        }

        if add_introduced_dependencies && (needs_new_unit || has_new_unit) {
            let introduced = collect_introduced_dependencies(
                project_cache,
                delphi_cache.as_deref_mut(),
                &project_map,
                new_unit,
                &mut summary.warnings,
            )?;
            if has_new_unit && last_inserted_name.is_none() {
                last_inserted_name = Some(new_unit.name.clone());
            }

            for dep_unit in introduced {
                if current_list
                    .entries
                    .iter()
                    .any(|entry| entry.name.eq_ignore_ascii_case(&dep_unit.name))
                {
                    continue;
                }

                let dep_insert_after = last_inserted_name.as_ref().and_then(|name| {
                    current_list.entries.iter().position(|entry| {
                        !entry.from_include && entry.name.eq_ignore_ascii_case(name)
                    })
                });
                let dep_updated = match insert_new_unit(
                    &current_bytes,
                    path,
                    &current_list,
                    &dep_unit,
                    dep_insert_after,
                ) {
                    Ok(value) => value,
                    Err(err) => {
                        summary.warnings.push(format!(
                            "warning: failed to update dpr {}: {err}",
                            path.display()
                        ));
                        summary.failures += 1;
                        continue 'dpr_loop;
                    }
                };
                if !dep_updated {
                    continue;
                }

                dpr_updated = true;
                last_inserted_name = Some(dep_unit.name);
                let reloaded = match reload_dpr_state(path, &mut summary.warnings) {
                    Ok(Some(value)) => value,
                    Ok(None) => {
                        summary
                            .warnings
                            .push(format!("warning: no uses list found in {}", path.display()));
                        summary.failures += 1;
                        continue 'dpr_loop;
                    }
                    Err(err) => {
                        summary.warnings.push(format!(
                            "warning: failed to read dpr {}: {err}",
                            path.display()
                        ));
                        summary.failures += 1;
                        continue 'dpr_loop;
                    }
                };
                current_bytes = reloaded.0;
                current_list = reloaded.1;
            }
        }

        if dpr_updated {
            summary.updated += 1;
            summary.updated_paths.push(path.clone());
        }
    }

    Ok(summary)
}

pub fn fix_dpr_file(
    dpr_path: &Path,
    project_cache: &UnitCache,
    delphi_cache: Option<&UnitCache>,
) -> io::Result<DprUpdateSummary> {
    let dpr_path = unit_cache::canonicalize_if_exists(dpr_path);
    let mut summary = DprUpdateSummary {
        scanned: 1,
        updated: 0,
        updated_paths: Vec::new(),
        warnings: Vec::new(),
        failures: 0,
    };

    let bytes = match fs::read(&dpr_path) {
        Ok(data) => data,
        Err(err) => {
            summary.warnings.push(format!(
                "warning: failed to read dpr {}: {err}",
                dpr_path.display()
            ));
            summary.failures += 1;
            return Ok(summary);
        }
    };
    let Some(list) = parse_dpr_uses(&dpr_path, &bytes, &mut summary.warnings) else {
        summary.warnings.push(format!(
            "warning: no uses list found in {}",
            dpr_path.display()
        ));
        summary.failures += 1;
        return Ok(summary);
    };
    let mut current_bytes = bytes;
    let mut current_list = list;
    let existing_names: HashSet<String> = current_list
        .entries
        .iter()
        .map(|entry| entry.name.to_ascii_lowercase())
        .collect();

    let project_map = build_project_map(
        &dpr_path,
        &current_list,
        project_cache,
        delphi_cache,
        &mut summary.warnings,
    );
    let root_paths = collect_fix_root_paths(
        &dpr_path,
        &current_list,
        &project_map,
        project_cache,
        delphi_cache,
        &mut summary.warnings,
    );
    if root_paths.is_empty() {
        return Ok(summary);
    }

    let missing_units = collect_missing_dpr_dependencies(
        &root_paths,
        &existing_names,
        project_cache,
        delphi_cache,
        &mut summary.warnings,
    );
    if missing_units.is_empty() {
        return Ok(summary);
    }

    let mut dpr_updated = false;
    let mut last_inserted_name = None::<String>;
    for dep_unit in missing_units {
        let dep_insert_after = last_inserted_name.as_ref().and_then(|name| {
            current_list
                .entries
                .iter()
                .position(|entry| !entry.from_include && entry.name.eq_ignore_ascii_case(name))
        });
        let dep_updated = match insert_new_unit(
            &current_bytes,
            &dpr_path,
            &current_list,
            &dep_unit,
            dep_insert_after,
        ) {
            Ok(value) => value,
            Err(err) => {
                summary.warnings.push(format!(
                    "warning: failed to update dpr {}: {err}",
                    dpr_path.display()
                ));
                summary.failures += 1;
                return Ok(summary);
            }
        };
        if !dep_updated {
            continue;
        }

        dpr_updated = true;
        last_inserted_name = Some(dep_unit.name);
        let reloaded = match reload_dpr_state(&dpr_path, &mut summary.warnings) {
            Ok(Some(value)) => value,
            Ok(None) => {
                summary.warnings.push(format!(
                    "warning: no uses list found in {}",
                    dpr_path.display()
                ));
                summary.failures += 1;
                return Ok(summary);
            }
            Err(err) => {
                summary.warnings.push(format!(
                    "warning: failed to read dpr {}: {err}",
                    dpr_path.display()
                ));
                summary.failures += 1;
                return Ok(summary);
            }
        };
        current_bytes = reloaded.0;
        current_list = reloaded.1;
    }

    if dpr_updated {
        summary.updated += 1;
        summary.updated_paths.push(dpr_path);
    }

    Ok(summary)
}

fn collect_fix_root_paths(
    dpr_path: &Path,
    list: &UsesList,
    project_map: &HashMap<String, PathBuf>,
    project_cache: &UnitCache,
    delphi_cache: Option<&UnitCache>,
    warnings: &mut Vec<String>,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();

    for entry in &list.entries {
        let key = entry.name.to_ascii_lowercase();
        let Some(path) = project_map.get(&key) else {
            continue;
        };
        let canonical = unit_cache::canonicalize_if_exists(path);
        if !has_unit_path(project_cache, delphi_cache, &canonical) {
            warnings.push(format!(
                "warning: unit {} in {} resolved outside known unit caches and will be ignored",
                entry.name,
                dpr_path.display()
            ));
            continue;
        }
        if seen.insert(canonical.clone()) {
            roots.push(canonical);
        }
    }

    roots
}

fn collect_missing_dpr_dependencies(
    root_paths: &[PathBuf],
    existing_names: &HashSet<String>,
    project_cache: &UnitCache,
    delphi_cache: Option<&UnitCache>,
    warnings: &mut Vec<String>,
) -> Vec<UnitFileInfo> {
    let mut queue = VecDeque::new();
    let mut seen_paths = HashSet::new();
    let mut missing_names = HashSet::new();
    let mut missing_units = Vec::new();

    for path in root_paths {
        if seen_paths.insert(path.clone()) {
            queue.push_back(path.clone());
        }
    }

    while let Some(unit_path) = queue.pop_front() {
        let Some(unit_info) = lookup_unit_info(project_cache, delphi_cache, &unit_path) else {
            continue;
        };

        for dep in &unit_info.uses {
            let dep_key = dep.to_ascii_lowercase();
            let dep_path = match resolve_by_name(project_cache, delphi_cache, dep.as_str()) {
                ResolveByName::Unique { path, .. } => path,
                ResolveByName::Ambiguous { count, source } => {
                    warnings.push(format!(
                        "warning: ambiguous unit {} referenced by {} ({} {} matches)",
                        dep,
                        unit_path.display(),
                        count,
                        source_label(source)
                    ));
                    continue;
                }
                ResolveByName::NotFound => continue,
            };
            let dep_path = unit_cache::canonicalize_if_exists(&dep_path);
            if !has_unit_path(project_cache, delphi_cache, &dep_path) {
                continue;
            }
            if seen_paths.insert(dep_path.clone()) {
                queue.push_back(dep_path.clone());
            }

            if existing_names.contains(&dep_key) {
                continue;
            }
            if !missing_names.insert(dep_key) {
                continue;
            }
            if let Some(dep_info) = lookup_unit_info(project_cache, delphi_cache, &dep_path) {
                missing_units.push(dep_info.clone());
            }
        }
    }

    missing_units
}

fn reload_dpr_state(
    path: &Path,
    warnings: &mut Vec<String>,
) -> io::Result<Option<(Vec<u8>, UsesList)>> {
    let bytes = fs::read(path)?;
    let list = parse_dpr_uses(path, &bytes, warnings);
    Ok(list.map(|list| (bytes, list)))
}

fn has_unit_path(project_cache: &UnitCache, delphi_cache: Option<&UnitCache>, path: &Path) -> bool {
    if project_cache.by_path.contains_key(path) {
        return true;
    }
    if let Some(delphi_cache) = delphi_cache {
        if delphi_cache.by_path.contains_key(path) {
            return true;
        }
    }
    false
}

fn lookup_unit_info<'a>(
    project_cache: &'a UnitCache,
    delphi_cache: Option<&'a UnitCache>,
    path: &Path,
) -> Option<&'a UnitFileInfo> {
    if let Some(unit) = project_cache.by_path.get(path) {
        return Some(unit);
    }
    delphi_cache.and_then(|cache| cache.by_path.get(path))
}

fn find_direct_introducer_index(
    list: &UsesList,
    project_map: &HashMap<String, PathBuf>,
    dependents: &ProjectDependents,
) -> Option<usize> {
    list.entries.iter().enumerate().find_map(|(idx, entry)| {
        if entry.from_include {
            return None;
        }
        let key = entry.name.to_ascii_lowercase();
        let path = project_map.get(&key)?;
        let id = *dependents.id_by_path.get(path)?;
        if dependents.direct[id] {
            Some(idx)
        } else {
            None
        }
    })
}

struct ProjectDependents {
    dependents: Vec<bool>,
    direct: Vec<bool>,
    id_by_path: HashMap<PathBuf, usize>,
}

fn build_project_map(
    dpr_path: &Path,
    list: &UsesList,
    project_cache: &UnitCache,
    delphi_cache: Option<&UnitCache>,
    warnings: &mut Vec<String>,
) -> HashMap<String, PathBuf> {
    let mut map = HashMap::new();

    for entry in &list.entries {
        let Some(raw_path) = entry.in_path.as_ref() else {
            match resolve_by_name(project_cache, delphi_cache, &entry.name) {
                ResolveByName::NotFound => {}
                ResolveByName::Unique {
                    path: fallback,
                    source,
                } => {
                    if source == ResolutionSource::Project {
                        warnings.push(format!(
                            "warning: missing in-path for unit {} in {} (resolved via scan)",
                            entry.name,
                            dpr_path.display()
                        ));
                    }
                    insert_project_entry(&mut map, entry, fallback, dpr_path, warnings);
                }
                ResolveByName::Ambiguous { count, source } => {
                    warnings.push(format!(
                        "warning: missing in-path for unit {} in {} ({} {} matches)",
                        entry.name,
                        dpr_path.display(),
                        count,
                        source_label(source)
                    ));
                }
            }
            continue;
        };

        let resolved = resolve_dpr_unit_path(dpr_path, raw_path);
        if !resolved.is_file() {
            warnings.push(format!(
                "warning: dpr uses path not found for unit {} in {}: {}",
                entry.name,
                dpr_path.display(),
                resolved.display()
            ));
            match resolve_by_name(project_cache, delphi_cache, &entry.name) {
                ResolveByName::Unique { path: fallback, .. } => {
                    insert_project_entry(&mut map, entry, fallback, dpr_path, warnings);
                }
                ResolveByName::Ambiguous { count, source } => {
                    warnings.push(format!(
                        "warning: unit {} referenced in {} is ambiguous ({} {} matches)",
                        entry.name,
                        dpr_path.display(),
                        count,
                        source_label(source)
                    ));
                }
                ResolveByName::NotFound => {}
            }
            continue;
        }

        insert_project_entry(&mut map, entry, resolved, dpr_path, warnings);
    }

    map
}

fn insert_project_entry(
    map: &mut HashMap<String, PathBuf>,
    entry: &UsesEntry,
    resolved: PathBuf,
    dpr_path: &Path,
    warnings: &mut Vec<String>,
) {
    let key = entry.name.to_ascii_lowercase();
    if let Some(existing) = map.get(&key) {
        if existing != &resolved {
            warnings.push(format!(
                "warning: duplicate unit name {} in {} with multiple paths",
                entry.name,
                dpr_path.display()
            ));
        }
        return;
    }
    map.insert(key, resolved);
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ResolutionSource {
    Project,
    Delphi,
}

enum ResolveByName {
    NotFound,
    Unique {
        path: PathBuf,
        source: ResolutionSource,
    },
    Ambiguous {
        count: usize,
        source: ResolutionSource,
    },
}

fn resolve_by_name(
    project_cache: &UnitCache,
    delphi_cache: Option<&UnitCache>,
    unit_name: &str,
) -> ResolveByName {
    let key = unit_name.to_ascii_lowercase();
    if let Some(paths) = project_cache.by_name.get(&key) {
        if paths.len() > 1 {
            return ResolveByName::Ambiguous {
                count: paths.len(),
                source: ResolutionSource::Project,
            };
        }
        return ResolveByName::Unique {
            path: paths[0].clone(),
            source: ResolutionSource::Project,
        };
    }

    if let Some(delphi_cache) = delphi_cache {
        if let Some(paths) = delphi_cache.by_name.get(&key) {
            if paths.len() > 1 {
                return ResolveByName::Ambiguous {
                    count: paths.len(),
                    source: ResolutionSource::Delphi,
                };
            }
            return ResolveByName::Unique {
                path: paths[0].clone(),
                source: ResolutionSource::Delphi,
            };
        }
    }

    ResolveByName::NotFound
}

fn source_label(source: ResolutionSource) -> &'static str {
    match source {
        ResolutionSource::Project => "project",
        ResolutionSource::Delphi => "--delphi-path",
    }
}

fn compute_project_dependents(
    project_cache: &mut UnitCache,
    mut delphi_cache: Option<&mut UnitCache>,
    project_map: &HashMap<String, PathBuf>,
    new_unit: &UnitFileInfo,
    warnings: &mut Vec<String>,
) -> io::Result<ProjectDependents> {
    let mut id_by_path = HashMap::new();
    let mut rev: Vec<Vec<usize>> = Vec::new();
    let mut direct: Vec<bool> = Vec::new();
    let mut queue = VecDeque::new();

    for path in project_map.values() {
        if id_by_path.contains_key(path) {
            continue;
        }
        let id = id_by_path.len();
        id_by_path.insert(path.clone(), id);
        rev.push(Vec::new());
        direct.push(false);
        queue.push_back(path.clone());
    }

    while let Some(unit_path) = queue.pop_front() {
        let uses = match load_unit_uses(
            project_cache,
            delphi_cache.as_deref_mut(),
            &unit_path,
            warnings,
        )? {
            Some(uses) => uses,
            None => {
                warnings.push(format!(
                    "warning: failed to read unit at {}",
                    unit_path.display()
                ));
                continue;
            }
        };
        let Some(&source_id) = id_by_path.get(&unit_path) else {
            continue;
        };

        for dep in uses {
            if dep.eq_ignore_ascii_case(&new_unit.name) {
                direct[source_id] = true;
                continue;
            }
            let dep_path = resolve_dep_path(
                project_map,
                project_cache,
                delphi_cache.as_deref(),
                dep.as_str(),
                unit_path.as_path(),
                warnings,
            );
            let Some(dep_path) = dep_path else {
                continue;
            };
            let target_id = if let Some(&id) = id_by_path.get(&dep_path) {
                id
            } else {
                let id = id_by_path.len();
                id_by_path.insert(dep_path.clone(), id);
                rev.push(Vec::new());
                direct.push(false);
                queue.push_back(dep_path.clone());
                id
            };
            rev[target_id].push(source_id);
        }
    }

    let mut dependents = vec![false; id_by_path.len()];
    let mut queue = VecDeque::new();
    for (id, is_direct) in direct.iter().copied().enumerate() {
        if is_direct {
            dependents[id] = true;
            queue.push_back(id);
        }
    }

    while let Some(current) = queue.pop_front() {
        for &next in &rev[current] {
            if !dependents[next] {
                dependents[next] = true;
                queue.push_back(next);
            }
        }
    }

    Ok(ProjectDependents {
        dependents,
        direct,
        id_by_path,
    })
}

fn resolve_dep_path(
    project_map: &HashMap<String, PathBuf>,
    project_cache: &UnitCache,
    delphi_cache: Option<&UnitCache>,
    dep_name: &str,
    source_path: &Path,
    warnings: &mut Vec<String>,
) -> Option<PathBuf> {
    let dep_key = dep_name.to_ascii_lowercase();
    if let Some(path) = project_map.get(&dep_key) {
        return Some(path.clone());
    }
    match resolve_by_name(project_cache, delphi_cache, dep_name) {
        ResolveByName::Unique { path, .. } => Some(path),
        ResolveByName::Ambiguous { count, source } => {
            warnings.push(format!(
                "warning: ambiguous unit {} referenced by {} ({} {} matches)",
                dep_name,
                source_path.display(),
                count,
                source_label(source)
            ));
            None
        }
        ResolveByName::NotFound => None,
    }
}

fn load_unit_uses(
    project_cache: &mut UnitCache,
    delphi_cache: Option<&mut UnitCache>,
    unit_path: &Path,
    warnings: &mut Vec<String>,
) -> io::Result<Option<Vec<String>>> {
    let canonical = unit_cache::canonicalize_if_exists(unit_path);
    if let Some(info) = project_cache.by_path.get(&canonical) {
        return Ok(Some(info.uses.clone()));
    }

    if let Some(delphi_cache) = delphi_cache {
        if let Some(info) = delphi_cache.by_path.get(&canonical) {
            return Ok(Some(info.uses.clone()));
        }
    }

    Ok(unit_cache::load_unit_file(&canonical, warnings)?.map(|info| info.uses))
}

fn collect_introduced_dependencies(
    project_cache: &mut UnitCache,
    mut delphi_cache: Option<&mut UnitCache>,
    project_map: &HashMap<String, PathBuf>,
    new_unit: &UnitFileInfo,
    warnings: &mut Vec<String>,
) -> io::Result<Vec<UnitFileInfo>> {
    let mut queue = VecDeque::new();
    let mut seen_paths = HashSet::new();
    let mut seen_names = HashSet::new();
    let mut introduced = Vec::new();

    let root_path = unit_cache::canonicalize_if_exists(&new_unit.path);
    seen_paths.insert(root_path.clone());
    queue.push_back(root_path.clone());

    while let Some(unit_path) = queue.pop_front() {
        let uses = match load_unit_uses(
            project_cache,
            delphi_cache.as_deref_mut(),
            &unit_path,
            warnings,
        )? {
            Some(uses) => uses,
            None => {
                warnings.push(format!(
                    "warning: failed to read unit at {}",
                    unit_path.display()
                ));
                continue;
            }
        };

        for dep in uses {
            if dep.eq_ignore_ascii_case(&new_unit.name) {
                continue;
            }
            let dep_path = resolve_dep_path(
                project_map,
                project_cache,
                delphi_cache.as_deref(),
                dep.as_str(),
                unit_path.as_path(),
                warnings,
            );
            let Some(dep_path) = dep_path else {
                continue;
            };
            let dep_path = unit_cache::canonicalize_if_exists(&dep_path);
            if dep_path == root_path {
                continue;
            }
            if seen_paths.insert(dep_path.clone()) {
                queue.push_back(dep_path.clone());
            }

            let dep_key = dep.to_ascii_lowercase();
            if !seen_names.insert(dep_key) {
                continue;
            }
            introduced.push(UnitFileInfo {
                name: dep,
                path: dep_path,
                uses: Vec::new(),
            });
        }
    }

    Ok(introduced)
}

fn resolve_dpr_unit_path(dpr_path: &Path, raw: &str) -> PathBuf {
    let candidate = PathBuf::from(raw);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        dpr_path
            .parent()
            .map(|parent| parent.join(&candidate))
            .unwrap_or(candidate)
    };
    unit_cache::canonicalize_if_exists(&resolved)
}

fn insert_new_unit(
    bytes: &[u8],
    dpr_path: &Path,
    list: &UsesList,
    new_unit: &UnitFileInfo,
    insert_after: Option<usize>,
) -> io::Result<bool> {
    let rel_path = relative_path(&new_unit.path, dpr_path.parent());
    let separator = if list.has_backslash {
        '\\'
    } else if list.has_slash {
        '/'
    } else {
        '\\'
    };
    let separator_str = separator.to_string();
    let rel_path = rel_path.replace(['\\', '/'], &separator_str);
    let entry_text = format!("{} in '{}'", new_unit.name, rel_path);

    if let Some(idx) = insert_after {
        if let Some((insert_at, insert_bytes)) =
            build_insertion_after(bytes, list, idx, entry_text.as_bytes())
        {
            let mut output = Vec::with_capacity(bytes.len() + insert_bytes.len());
            output.extend_from_slice(&bytes[..insert_at]);
            output.extend_from_slice(&insert_bytes);
            output.extend_from_slice(&bytes[insert_at..]);
            write_atomic(dpr_path, &output)?;
            return Ok(true);
        }
    }

    let line_ending = detect_line_ending(bytes);
    let last_delim = list.entries.last().and_then(|entry| entry.delimiter);
    let insertion = if list.multiline {
        let prefix = if matches!(last_delim, Some(b',')) {
            ""
        } else {
            ","
        };
        format!("{prefix}{line_ending}{}{}", list.indent, entry_text)
    } else {
        let prefix = if matches!(last_delim, Some(b',')) {
            " "
        } else {
            ", "
        };
        format!("{prefix}{entry_text}")
    };

    let insert_at = if list.multiline && !matches!(last_delim, Some(b',')) {
        let mut pos = list.semicolon;
        while pos > 0 && bytes[pos - 1].is_ascii_whitespace() {
            pos -= 1;
        }
        pos
    } else {
        list.semicolon
    };

    let insert_bytes = insertion.as_bytes();
    let mut output = Vec::with_capacity(bytes.len() + insert_bytes.len());
    output.extend_from_slice(&bytes[..insert_at]);
    output.extend_from_slice(insert_bytes);
    output.extend_from_slice(&bytes[insert_at..]);

    write_atomic(dpr_path, &output)?;
    Ok(true)
}

fn build_insertion_after(
    bytes: &[u8],
    list: &UsesList,
    insert_after: usize,
    entry_text: &[u8],
) -> Option<(usize, Vec<u8>)> {
    let entry = list.entries.get(insert_after)?;
    if entry.from_include {
        return None;
    }
    let delimiter_pos = entry.delimiter_pos?;
    if entry.delimiter != Some(b',') {
        return None;
    }
    let next_entry = list.entries.get(insert_after + 1)?;
    let next_start = next_entry.start;
    if delimiter_pos + 1 > next_start || next_start > bytes.len() {
        return None;
    }

    let separator_after = &bytes[delimiter_pos + 1..next_start];
    let separator_before = separator_before_new_entry(bytes, list, separator_after);

    let mut insertion = Vec::new();
    insertion.extend_from_slice(&separator_before);
    insertion.extend_from_slice(entry_text);
    insertion.push(b',');

    Some((delimiter_pos + 1, insertion))
}

fn separator_before_new_entry<'a>(
    bytes: &[u8],
    list: &UsesList,
    separator_after: &'a [u8],
) -> std::borrow::Cow<'a, [u8]> {
    if separator_after
        .iter()
        .all(|byte| byte.is_ascii_whitespace())
    {
        return std::borrow::Cow::Borrowed(separator_after);
    }

    let leading_ws_len = separator_after
        .iter()
        .take_while(|byte| byte.is_ascii_whitespace())
        .count();
    if leading_ws_len > 0 {
        return std::borrow::Cow::Borrowed(&separator_after[..leading_ws_len]);
    }

    let line_ending = detect_line_ending(bytes);
    let fallback = if list.multiline {
        format!("{line_ending}{}", list.indent)
    } else {
        " ".to_string()
    };
    std::borrow::Cow::Owned(fallback.into_bytes())
}

fn relative_path(target: &Path, base: Option<&Path>) -> String {
    if let Some(base) = base {
        if let Some(diff) = pathdiff::diff_paths(target, base) {
            return diff.to_string_lossy().to_string();
        }
    }
    target.to_string_lossy().to_string()
}

fn parse_dpr_uses(dpr_path: &Path, bytes: &[u8], warnings: &mut Vec<String>) -> Option<UsesList> {
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => i = pas_lex::skip_brace_comment(bytes, i + 1),
            b'(' if bytes.get(i + 1) == Some(&b'*') => {
                i = pas_lex::skip_paren_comment(bytes, i + 2)
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = pas_lex::skip_line_comment(bytes, i + 2),
            b'\'' => i = pas_lex::skip_string(bytes, i + 1),
            byte if pas_lex::is_ident_start(byte) => {
                let (token, next) = pas_lex::read_ident(bytes, i);
                if token.eq_ignore_ascii_case("uses") {
                    return parse_dpr_uses_list(dpr_path, bytes, next, warnings);
                }
                i = next;
            }
            _ => i += 1,
        }
    }
    None
}

fn parse_dpr_uses_list(
    dpr_path: &Path,
    bytes: &[u8],
    i: usize,
    warnings: &mut Vec<String>,
) -> Option<UsesList> {
    let list_start = i;
    let mut entries = Vec::new();
    let mut has_backslash = false;
    let mut has_slash = false;
    let mut include_semicolon = false;
    let mut include_stack = Vec::new();
    include_stack.push(unit_cache::canonicalize_if_exists(dpr_path));
    let mut state = DprParseState {
        warnings,
        include_stack: &mut include_stack,
        has_backslash: &mut has_backslash,
        has_slash: &mut has_slash,
        include_semicolon: &mut include_semicolon,
    };

    let semicolon =
        parse_uses_fragment_for_dpr(bytes, i, dpr_path, &mut entries, &mut state, None)?;
    if include_semicolon {
        return None;
    }
    if entries.is_empty() {
        return None;
    }
    let multiline = bytes[list_start..semicolon].contains(&b'\n');
    let indent = if multiline {
        entries
            .first()
            .map(|entry| infer_indent(bytes, entry.start))
            .unwrap_or_default()
    } else {
        String::new()
    };

    Some(UsesList {
        entries,
        semicolon,
        multiline,
        indent,
        has_backslash,
        has_slash,
    })
}

struct DprParseState<'a> {
    warnings: &'a mut Vec<String>,
    include_stack: &'a mut Vec<PathBuf>,
    has_backslash: &'a mut bool,
    has_slash: &'a mut bool,
    include_semicolon: &'a mut bool,
}

fn parse_uses_fragment_for_dpr(
    bytes: &[u8],
    mut i: usize,
    source_path: &Path,
    entries: &mut Vec<UsesEntry>,
    state: &mut DprParseState<'_>,
    entry_start_override: Option<usize>,
) -> Option<usize> {
    while i < bytes.len() {
        i = skip_ws_comments_and_includes_dpr(
            bytes,
            i,
            source_path,
            entries,
            state,
            entry_start_override,
        );
        if i >= bytes.len() {
            return None;
        }
        if bytes[i] == b';' {
            if entry_start_override.is_some() {
                state.warnings.push(format!(
                    "warning: include file {} contains ';' in uses list",
                    source_path.display()
                ));
                *state.include_semicolon = true;
            }
            return Some(i);
        }
        if !pas_lex::is_ident_start(bytes[i]) {
            i += 1;
            continue;
        }

        let entry_start = i;
        let (name, next) = pas_lex::read_ident_with_dots(bytes, i);
        i = next;
        i = pas_lex::skip_ws_and_comments(bytes, i);

        let mut in_path = None;
        if let Some((token, next_token)) = peek_ident(bytes, i) {
            if token.eq_ignore_ascii_case("in") {
                i = next_token;
                i = skip_ws_and_comments_no_strings(bytes, i);
                if i < bytes.len() && bytes[i] == b'\'' {
                    if let Some((value, end)) = pas_lex::read_string_literal(bytes, i) {
                        in_path = Some(value);
                        i = end;
                    } else {
                        i = pas_lex::skip_string(bytes, i + 1);
                    }
                }
            }
        }

        update_path_separator_flags(&in_path, state.has_backslash, state.has_slash);

        let (pos, delim, include_entries) =
            scan_to_delimiter_with_includes(bytes, i, source_path, state, entry_start_override);
        let start = entry_start_override.unwrap_or(entry_start);
        entries.push(UsesEntry {
            name,
            in_path,
            start,
            delimiter: delim,
            delimiter_pos: if entry_start_override.is_some() {
                None
            } else {
                delim.map(|_| pos)
            },
            from_include: entry_start_override.is_some(),
        });
        if !include_entries.is_empty() {
            entries.extend(include_entries);
        }
        match delim {
            Some(b',') => i = pos + 1,
            Some(b';') => return Some(pos),
            _ => return None,
        }
    }
    None
}

fn skip_ws_comments_and_includes_dpr(
    bytes: &[u8],
    mut i: usize,
    source_path: &Path,
    entries: &mut Vec<UsesEntry>,
    state: &mut DprParseState<'_>,
    entry_start_override: Option<usize>,
) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'{' | b'(' => {
                if let Some((include_name, end)) = pas_lex::parse_include_directive(bytes, i) {
                    let anchor = entry_start_override.unwrap_or(i);
                    let include_entries = parse_include_entries_for_dpr(
                        include_name.as_str(),
                        anchor,
                        source_path,
                        state,
                    );
                    if !include_entries.is_empty() {
                        entries.extend(include_entries);
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

fn scan_to_delimiter_with_includes(
    bytes: &[u8],
    mut i: usize,
    source_path: &Path,
    state: &mut DprParseState<'_>,
    entry_start_override: Option<usize>,
) -> (usize, Option<u8>, Vec<UsesEntry>) {
    let mut include_entries = Vec::new();
    while i < bytes.len() {
        match bytes[i] {
            b',' | b';' => return (i, Some(bytes[i]), include_entries),
            b'{' | b'(' => {
                if let Some((include_name, end)) = pas_lex::parse_include_directive(bytes, i) {
                    let anchor = entry_start_override.unwrap_or(i);
                    let entries = parse_include_entries_for_dpr(
                        include_name.as_str(),
                        anchor,
                        source_path,
                        state,
                    );
                    if !entries.is_empty() {
                        include_entries.extend(entries);
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
    (i, None, include_entries)
}

fn parse_include_entries_for_dpr(
    include_name: &str,
    anchor: usize,
    source_path: &Path,
    state: &mut DprParseState<'_>,
) -> Vec<UsesEntry> {
    let DprParseState {
        warnings,
        include_stack,
        has_backslash,
        has_slash,
        include_semicolon,
    } = &mut *state;

    uses_include::with_include_bytes(
        include_name,
        source_path,
        warnings,
        include_stack,
        |include_path, bytes, warnings, include_stack| {
            let mut entries = Vec::new();
            let mut nested_state = DprParseState {
                warnings,
                include_stack,
                has_backslash,
                has_slash,
                include_semicolon,
            };
            let _ = parse_uses_fragment_for_dpr(
                bytes,
                0,
                include_path,
                &mut entries,
                &mut nested_state,
                Some(anchor),
            );
            entries
        },
    )
    .unwrap_or_default()
}

fn peek_ident(bytes: &[u8], i: usize) -> Option<(String, usize)> {
    if i < bytes.len() && pas_lex::is_ident_start(bytes[i]) {
        let (token, next) = pas_lex::read_ident(bytes, i);
        return Some((token, next));
    }
    None
}

fn update_path_separator_flags(
    in_path: &Option<String>,
    has_backslash: &mut bool,
    has_slash: &mut bool,
) {
    let Some(path) = in_path.as_ref() else {
        return;
    };
    if path.contains('\\') {
        *has_backslash = true;
    }
    if path.contains('/') {
        *has_slash = true;
    }
}

fn skip_ws_and_comments_no_strings(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'{' => i = pas_lex::skip_brace_comment(bytes, i + 1),
            b'(' if bytes.get(i + 1) == Some(&b'*') => {
                i = pas_lex::skip_paren_comment(bytes, i + 2)
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => i = pas_lex::skip_line_comment(bytes, i + 2),
            _ => break,
        }
    }
    i
}

fn infer_indent(bytes: &[u8], entry_start: usize) -> String {
    let line_start = bytes[..entry_start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|pos| pos + 1)
        .unwrap_or(0);
    let indent_bytes = &bytes[line_start..entry_start];
    let indent = indent_bytes
        .iter()
        .take_while(|&&b| b == b' ' || b == b'\t')
        .copied()
        .collect::<Vec<_>>();
    String::from_utf8_lossy(&indent).to_string()
}

fn detect_line_ending(bytes: &[u8]) -> &'static str {
    if bytes.windows(2).any(|pair| pair == b"\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, contents)?;
    match fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(path)?;
            fs::rename(temp_path, path)
        }
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parse_dpr_uses_single_line() {
        let src = b"program Demo;\nuses Foo, Bar;\nbegin end.";
        let root = temp_dir();
        let dpr_path = root.join("Demo.dpr");
        let mut warnings = Vec::new();
        let list = parse_dpr_uses(&dpr_path, src, &mut warnings).expect("uses list");
        assert_eq!(list.entries.len(), 2);
        assert_eq!(list.entries[0].name, "Foo");
        assert_eq!(list.entries[1].name, "Bar");
        assert!(list.entries[0].in_path.is_none());
        assert!(list.entries[1].in_path.is_none());
        assert!(!list.multiline);
        assert!(list.indent.is_empty());
    }

    #[test]
    fn parse_dpr_uses_multiline_with_indent_and_paths() {
        let src = b"program Demo;\nuses\n  Foo,\n  Bar in 'lib\\Bar.pas',\n  Baz;\nbegin end.";
        let root = temp_dir();
        let dpr_path = root.join("Demo.dpr");
        let mut warnings = Vec::new();
        let list = parse_dpr_uses(&dpr_path, src, &mut warnings).expect("uses list");
        assert_eq!(list.entries.len(), 3);
        assert!(list.multiline);
        assert_eq!(list.indent, "  ");
        assert!(
            list.has_backslash,
            "expected backslash path detection, list={list:?}"
        );
        assert!(!list.has_slash);
        assert_eq!(list.entries[1].in_path.as_deref(), Some("lib\\Bar.pas"));
    }

    #[test]
    fn parse_dpr_uses_ignores_comments_and_directives() {
        let src = br#"
program Demo;
uses Foo, {Bar}, (*Baz*), {$IFDEF X} Qux, {$ENDIF} RealUnit;
begin end.
"#;
        let root = temp_dir();
        let dpr_path = root.join("Demo.dpr");
        let mut warnings = Vec::new();
        let list = parse_dpr_uses(&dpr_path, src, &mut warnings).expect("uses list");
        let names: Vec<String> = list
            .entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect();
        assert_eq!(names, vec!["Foo", "Qux", "RealUnit"]);
        assert!(list.entries.iter().all(|entry| entry.in_path.is_none()));
    }

    #[test]
    fn insert_new_unit_single_line() {
        let root = temp_dir();
        let dpr_path = root.join("Demo.dpr");
        let pas_path = root.join("NewUnit.pas");
        fs::write(&dpr_path, "program Demo;\nuses Foo, Bar;\nbegin end.").unwrap();
        fs::write(&pas_path, "unit NewUnit;\ninterface\nend.").unwrap();

        let bytes = fs::read(&dpr_path).unwrap();
        let mut warnings = Vec::new();
        let list = parse_dpr_uses(&dpr_path, &bytes, &mut warnings).expect("uses list");
        let new_unit = UnitFileInfo {
            name: "NewUnit".to_string(),
            path: pas_path.clone(),
            uses: Vec::new(),
        };
        insert_new_unit(&bytes, &dpr_path, &list, &new_unit, None).unwrap();

        let updated = fs::read_to_string(&dpr_path).unwrap();
        assert!(
            updated.contains("uses Foo, Bar, NewUnit in 'NewUnit.pas';"),
            "{updated}"
        );
    }

    #[test]
    fn insert_new_unit_multiline_keeps_indent_and_separator() {
        let root = temp_dir();
        let dpr_path = root.join("Demo.dpr");
        let pas_dir = root.join("sub");
        fs::create_dir_all(&pas_dir).unwrap();
        let pas_path = pas_dir.join("NewUnit.pas");
        fs::write(
            &dpr_path,
            "program Demo;\r\nuses\r\n  Foo,\r\n  Bar in 'lib/Bar.pas',\r\n  Baz;\r\nbegin end.",
        )
        .unwrap();
        fs::write(&pas_path, "unit NewUnit;\ninterface\nend.").unwrap();

        let bytes = fs::read(&dpr_path).unwrap();
        let mut warnings = Vec::new();
        let list = parse_dpr_uses(&dpr_path, &bytes, &mut warnings).expect("uses list");
        let new_unit = UnitFileInfo {
            name: "NewUnit".to_string(),
            path: pas_path.clone(),
            uses: Vec::new(),
        };
        insert_new_unit(&bytes, &dpr_path, &list, &new_unit, None).unwrap();

        let updated = fs::read_to_string(&dpr_path).unwrap();
        assert!(
            updated.contains("Baz,\r\n  NewUnit in 'sub/NewUnit.pas';"),
            "{updated}"
        );
    }

    #[test]
    fn insert_new_unit_after_entry_single_line() {
        let root = temp_dir();
        let dpr_path = root.join("Demo.dpr");
        let pas_path = root.join("NewUnit.pas");
        fs::write(&dpr_path, "program Demo;\nuses Foo, Bar, Baz;\nbegin end.").unwrap();
        fs::write(&pas_path, "unit NewUnit;\ninterface\nend.").unwrap();

        let bytes = fs::read(&dpr_path).unwrap();
        let mut warnings = Vec::new();
        let list = parse_dpr_uses(&dpr_path, &bytes, &mut warnings).expect("uses list");
        let insert_after = list
            .entries
            .iter()
            .position(|entry| entry.name == "Bar")
            .expect("Bar entry");
        let new_unit = UnitFileInfo {
            name: "NewUnit".to_string(),
            path: pas_path.clone(),
            uses: Vec::new(),
        };
        insert_new_unit(&bytes, &dpr_path, &list, &new_unit, Some(insert_after)).unwrap();

        let updated = fs::read_to_string(&dpr_path).unwrap();
        assert!(
            updated.contains("uses Foo, Bar, NewUnit in 'NewUnit.pas', Baz;"),
            "{updated}"
        );
    }

    #[test]
    fn insert_new_unit_after_entry_multiline() {
        let root = temp_dir();
        let dpr_path = root.join("Demo.dpr");
        let pas_path = root.join("NewUnit.pas");
        fs::write(
            &dpr_path,
            "program Demo;\r\nuses\r\n  Foo,\r\n  Bar,\r\n  Baz;\r\nbegin end.",
        )
        .unwrap();
        fs::write(&pas_path, "unit NewUnit;\ninterface\nend.").unwrap();

        let bytes = fs::read(&dpr_path).unwrap();
        let mut warnings = Vec::new();
        let list = parse_dpr_uses(&dpr_path, &bytes, &mut warnings).expect("uses list");
        let insert_after = list
            .entries
            .iter()
            .position(|entry| entry.name == "Bar")
            .expect("Bar entry");
        let new_unit = UnitFileInfo {
            name: "NewUnit".to_string(),
            path: pas_path.clone(),
            uses: Vec::new(),
        };
        insert_new_unit(&bytes, &dpr_path, &list, &new_unit, Some(insert_after)).unwrap();

        let updated = fs::read_to_string(&dpr_path).unwrap();
        assert!(
            updated.contains("Bar,\r\n  NewUnit in 'NewUnit.pas',\r\n  Baz;"),
            "{updated}"
        );
    }

    #[test]
    fn parse_dpr_uses_semicolon_on_own_line() {
        let src = b"program Demo;\nuses\n  Foo,\n  Bar\n;\nbegin end.";
        let root = temp_dir();
        let dpr_path = root.join("Demo.dpr");
        let mut warnings = Vec::new();
        let list = parse_dpr_uses(&dpr_path, src, &mut warnings).expect("uses list");
        let names: Vec<String> = list
            .entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect();
        assert_eq!(names, vec!["Foo", "Bar"]);
        assert!(list.multiline);
        assert_eq!(list.indent, "  ");
    }

    #[test]
    fn parse_dpr_uses_mixed_separators_prefers_existing() {
        let src = b"program Demo;\nuses Foo in 'lib/Foo.pas', Bar in 'lib\\\\Bar.pas';\nbegin end.";
        let root = temp_dir();
        let dpr_path = root.join("Demo.dpr");
        let mut warnings = Vec::new();
        let list = parse_dpr_uses(&dpr_path, src, &mut warnings).expect("uses list");
        assert!(list.has_slash);
        assert!(list.has_backslash);
    }

    #[test]
    fn parse_dpr_uses_supports_include_fragments() {
        let root = temp_dir();
        let dpr_path = root.join("Demo.dpr");
        let include_path = root.join("Uses.inc");
        fs::write(
            &include_path,
            "Foo in 'lib\\\\Foo.pas',\nBar,\nBaz in 'lib/Baz.pas',",
        )
        .unwrap();
        let src = b"program Demo;\nuses\n  {$I Uses.inc}\n  Qux;\nbegin end.";
        let mut warnings = Vec::new();
        let list = parse_dpr_uses(&dpr_path, src, &mut warnings).expect("uses list");
        let names: Vec<String> = list
            .entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect();
        assert_eq!(names, vec!["Foo", "Bar", "Baz", "Qux"]);
        assert!(list.has_backslash);
        assert!(list.has_slash);
    }

    #[test]
    fn resolve_by_name_prefers_project_cache_before_delphi_cache() {
        let mut project_cache = UnitCache::default();
        let project_path = PathBuf::from(r"C:\project\Foo.pas");
        project_cache
            .by_name
            .insert("foo".to_string(), vec![project_path.clone()]);

        let mut delphi_cache = UnitCache::default();
        let delphi_path = PathBuf::from(r"C:\delphi\Foo.pas");
        delphi_cache
            .by_name
            .insert("foo".to_string(), vec![delphi_path.clone()]);

        match resolve_by_name(&project_cache, Some(&delphi_cache), "Foo") {
            ResolveByName::Unique { path, source } => {
                assert_eq!(path, project_path);
                assert_eq!(source, ResolutionSource::Project);
            }
            _ => panic!("expected unique project resolution"),
        }
    }

    #[test]
    fn resolve_by_name_uses_delphi_cache_when_project_missing() {
        let project_cache = UnitCache::default();
        let mut delphi_cache = UnitCache::default();
        let delphi_path = PathBuf::from(r"C:\delphi\ExtUnit.pas");
        delphi_cache
            .by_name
            .insert("extunit".to_string(), vec![delphi_path.clone()]);

        match resolve_by_name(&project_cache, Some(&delphi_cache), "ExtUnit") {
            ResolveByName::Unique { path, source } => {
                assert_eq!(path, delphi_path);
                assert_eq!(source, ResolutionSource::Delphi);
            }
            _ => panic!("expected unique delphi resolution"),
        }
    }

    #[test]
    fn collect_introduced_dependencies_returns_transitive_closure_without_root() {
        let root = temp_dir();
        let new_path = root.join("NewUnit.pas");
        let mid_path = root.join("MidUnit.pas");
        let base_path = root.join("BaseUnit.pas");
        fs::write(
            &new_path,
            "unit NewUnit;\ninterface\nuses MidUnit;\nimplementation\nend.\n",
        )
        .unwrap();
        fs::write(
            &mid_path,
            "unit MidUnit;\ninterface\nuses BaseUnit, NewUnit;\nimplementation\nend.\n",
        )
        .unwrap();
        fs::write(
            &base_path,
            "unit BaseUnit;\ninterface\nimplementation\nend.\n",
        )
        .unwrap();

        let mut warnings = Vec::new();
        let mut project_cache =
            unit_cache::build_unit_cache(&[new_path.clone(), mid_path, base_path], &mut warnings)
                .unwrap();
        let new_unit = unit_cache::load_unit_file(&new_path, &mut warnings)
            .unwrap()
            .expect("new unit");
        let project_map = HashMap::new();

        let introduced = collect_introduced_dependencies(
            &mut project_cache,
            None,
            &project_map,
            &new_unit,
            &mut warnings,
        )
        .unwrap();
        let names: Vec<String> = introduced
            .into_iter()
            .map(|unit| unit.name.to_ascii_lowercase())
            .collect();
        assert_eq!(names, vec!["midunit", "baseunit"]);
    }

    #[test]
    fn fix_dpr_file_adds_missing_transitive_dependencies_from_project_cache() {
        let root = temp_dir();
        let dpr_path = root.join("App.dpr");
        let unit_a = root.join("UnitA.pas");
        let unit_b = root.join("UnitB.pas");
        let unit_c = root.join("UnitC.pas");
        fs::write(
            &dpr_path,
            "program App;\nuses\n  UnitA in 'UnitA.pas';\nbegin\nend.\n",
        )
        .unwrap();
        fs::write(
            &unit_a,
            "unit UnitA;\ninterface\nuses UnitB;\nimplementation\nend.\n",
        )
        .unwrap();
        fs::write(
            &unit_b,
            "unit UnitB;\ninterface\nuses UnitC;\nimplementation\nend.\n",
        )
        .unwrap();
        fs::write(&unit_c, "unit UnitC;\ninterface\nimplementation\nend.\n").unwrap();

        let mut warnings = Vec::new();
        let cache = unit_cache::build_unit_cache(
            &[unit_a.clone(), unit_b.clone(), unit_c.clone()],
            &mut warnings,
        )
        .unwrap();

        let first = fix_dpr_file(&dpr_path, &cache, None).unwrap();
        assert_eq!(first.failures, 0, "{first:?}");
        assert_eq!(first.updated, 1, "{first:?}");
        let updated = fs::read_to_string(&dpr_path).unwrap();
        assert!(updated.contains("UnitB in 'UnitB.pas'"), "{updated}");
        assert!(updated.contains("UnitC in 'UnitC.pas'"), "{updated}");

        let second = fix_dpr_file(&dpr_path, &cache, None).unwrap();
        assert_eq!(second.failures, 0, "{second:?}");
        assert_eq!(second.updated, 0, "{second:?}");
    }

    #[test]
    fn fix_dpr_file_skips_dependencies_not_in_project_cache() {
        let root = temp_dir();
        let external = root.join("external");
        fs::create_dir_all(&external).unwrap();
        let dpr_path = root.join("App.dpr");
        let unit_a = root.join("UnitA.pas");
        let ext_unit = external.join("ExtUnit.pas");
        fs::write(
            &dpr_path,
            "program App;\nuses\n  UnitA in 'UnitA.pas';\nbegin\nend.\n",
        )
        .unwrap();
        fs::write(
            &unit_a,
            "unit UnitA;\ninterface\nuses ExtUnit;\nimplementation\nend.\n",
        )
        .unwrap();
        fs::write(
            &ext_unit,
            "unit ExtUnit;\ninterface\nimplementation\nend.\n",
        )
        .unwrap();

        let mut warnings = Vec::new();
        let cache =
            unit_cache::build_unit_cache(std::slice::from_ref(&unit_a), &mut warnings).unwrap();

        let result = fix_dpr_file(&dpr_path, &cache, None).unwrap();
        assert_eq!(result.failures, 0, "{result:?}");
        assert_eq!(result.updated, 0, "{result:?}");
        let updated = fs::read_to_string(&dpr_path).unwrap();
        assert!(!updated.contains("ExtUnit in "), "{updated}");
    }

    #[test]
    fn fix_dpr_file_uses_delphi_fallback_cache_when_provided() {
        let root = temp_dir();
        let external = root.join("delphi");
        fs::create_dir_all(&external).unwrap();
        let dpr_path = root.join("App.dpr");
        let unit_a = root.join("UnitA.pas");
        let ext_mid = external.join("ExtMid.pas");
        let new_unit = external.join("NewUnit.pas");
        fs::write(
            &dpr_path,
            "program App;\nuses\n  UnitA in 'UnitA.pas';\nbegin\nend.\n",
        )
        .unwrap();
        fs::write(
            &unit_a,
            "unit UnitA;\ninterface\nuses ExtMid;\nimplementation\nend.\n",
        )
        .unwrap();
        fs::write(
            &ext_mid,
            "unit ExtMid;\ninterface\nuses NewUnit;\nimplementation\nend.\n",
        )
        .unwrap();
        fs::write(
            &new_unit,
            "unit NewUnit;\ninterface\nimplementation\nend.\n",
        )
        .unwrap();

        let mut warnings = Vec::new();
        let project_cache =
            unit_cache::build_unit_cache(std::slice::from_ref(&unit_a), &mut warnings).unwrap();
        let delphi_cache =
            unit_cache::build_unit_cache(&[ext_mid, new_unit], &mut warnings).unwrap();

        let result = fix_dpr_file(&dpr_path, &project_cache, Some(&delphi_cache)).unwrap();
        assert_eq!(result.failures, 0, "{result:?}");
        assert_eq!(result.updated, 1, "{result:?}");
        let updated = fs::read_to_string(&dpr_path).unwrap();
        assert!(updated.contains("ExtMid in "), "{updated}");
        assert!(updated.contains("NewUnit in "), "{updated}");
    }

    fn temp_dir() -> PathBuf {
        let mut root = env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        root.push(format!("fixdpr_dpr_test_{nanos}"));
        fs::create_dir_all(&root).expect("create temp dir");
        root
    }
}
