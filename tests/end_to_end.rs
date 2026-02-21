use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn end_to_end_updates_expected_dprs() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_repo");
    let temp_root = temp_dir("fixdpr_e2e_");
    copy_dir(&fixture_root, &temp_root);

    let new_dependency = temp_root.join("common").join("NewUnit.pas");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("add-dependency")
        .arg("--search-path")
        .arg(&temp_root)
        .arg("--new-dependency")
        .arg(&new_dependency)
        .arg("--ignore-path")
        .arg(temp_root.join("ignored"))
        .output()
        .expect("run fixdpr");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let expected_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_expected");
    let expected_files = [
        PathBuf::from("app1").join("App1.dpr"),
        PathBuf::from("app2").join("App2.dpr"),
        PathBuf::from("app3").join("App3.dpr"),
        PathBuf::from("app4").join("App4.dpr"),
        PathBuf::from("ignored").join("Ignored.dpr"),
    ];

    for rel_path in expected_files {
        let actual_path = temp_root.join(&rel_path);
        let expected_path = expected_root.join(&rel_path);
        let actual = normalize_newlines(
            fs::read_to_string(&actual_path)
                .unwrap_or_else(|_| panic!("missing actual file: {}", actual_path.display())),
        );
        let expected = normalize_newlines(
            fs::read_to_string(&expected_path)
                .unwrap_or_else(|_| panic!("missing expected file: {}", expected_path.display())),
        );
        assert_eq!(actual, expected, "mismatch for {}", rel_path.display());
    }
}

#[test]
fn end_to_end_search_path_can_be_repeated_for_multiple_roots() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_repo");
    let expected_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_expected");
    let temp_root = temp_dir("fixdpr_e2e_multi_search_");
    copy_dir(&fixture_root, &temp_root);

    let new_dependency = temp_root.join("common").join("NewUnit.pas");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("add-dependency")
        .arg("--search-path")
        .arg(temp_root.join("app1"))
        .arg("--search-path")
        .arg(temp_root.join("app2"))
        .arg("--new-dependency")
        .arg(&new_dependency)
        .output()
        .expect("run fixdpr");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let app1_actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app1").join("App1.dpr")).expect("read app1 actual"),
    );
    let app1_expected = normalize_newlines(
        fs::read_to_string(expected_root.join("app1").join("App1.dpr"))
            .expect("read app1 expected"),
    );
    assert_eq!(app1_actual, app1_expected, "app1 should be updated");

    let app2_actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app2").join("App2.dpr")).expect("read app2 actual"),
    );
    let app2_expected = normalize_newlines(
        fs::read_to_string(expected_root.join("app2").join("App2.dpr"))
            .expect("read app2 expected"),
    );
    assert_eq!(app2_actual, app2_expected, "app2 should be updated");

    let app3_actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app3").join("App3.dpr")).expect("read app3 actual"),
    );
    let app3_expected = normalize_newlines(
        fs::read_to_string(fixture_root.join("app3").join("App3.dpr")).expect("read app3 expected"),
    );
    assert_eq!(app3_actual, app3_expected, "app3 should not be scanned");

    let app4_actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app4").join("App4.dpr")).expect("read app4 actual"),
    );
    let app4_expected = normalize_newlines(
        fs::read_to_string(fixture_root.join("app4").join("App4.dpr")).expect("read app4 expected"),
    );
    assert_eq!(app4_actual, app4_expected, "app4 should not be scanned");
}

#[test]
fn end_to_end_search_path_dedupes_overlapping_roots() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_repo");
    let expected_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_expected");
    let temp_root = temp_dir("fixdpr_e2e_glob_search_");
    copy_dir(&fixture_root, &temp_root);

    let new_dependency = temp_root.join("common").join("NewUnit.pas");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("add-dependency")
        .arg("--search-path")
        .arg(&temp_root)
        .arg("--search-path")
        .arg(temp_root.join("app1"))
        .arg("--new-dependency")
        .arg(&new_dependency)
        .arg("--ignore-path")
        .arg(temp_root.join("ignored"))
        .output()
        .expect("run fixdpr");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("dpr scanned: 4"), "{stdout}");

    let app1_actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app1").join("App1.dpr")).expect("read app1 actual"),
    );
    let app1_expected = normalize_newlines(
        fs::read_to_string(expected_root.join("app1").join("App1.dpr"))
            .expect("read app1 expected"),
    );
    assert_eq!(app1_actual, app1_expected, "app1 should be updated");

    let app2_actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app2").join("App2.dpr")).expect("read app2 actual"),
    );
    let app2_expected = normalize_newlines(
        fs::read_to_string(expected_root.join("app2").join("App2.dpr"))
            .expect("read app2 expected"),
    );
    assert_eq!(app2_actual, app2_expected, "app2 should be updated");

    let app3_actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app3").join("App3.dpr")).expect("read app3 actual"),
    );
    let app3_expected = normalize_newlines(
        fs::read_to_string(expected_root.join("app3").join("App3.dpr"))
            .expect("read app3 expected"),
    );
    assert_eq!(app3_actual, app3_expected, "app3 should be updated");

    let app4_actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app4").join("App4.dpr")).expect("read app4 actual"),
    );
    let app4_expected = normalize_newlines(
        fs::read_to_string(expected_root.join("app4").join("App4.dpr"))
            .expect("read app4 expected"),
    );
    assert_eq!(app4_actual, app4_expected, "app4 should be updated");
}

#[test]
fn end_to_end_search_path_requires_existing_directory() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_repo");
    let temp_root = temp_dir("fixdpr_e2e_search_warn_");
    copy_dir(&fixture_root, &temp_root);

    let matched_root = temp_root.clone();
    let missing_path = temp_root.join("missing");
    let new_dependency = temp_root.join("common").join("NewUnit.pas");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("add-dependency")
        .arg("--search-path")
        .arg(&matched_root)
        .arg("--search-path")
        .arg(&missing_path)
        .arg("--new-dependency")
        .arg(&new_dependency)
        .output()
        .expect("run fixdpr");

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--search-path does not exist"), "{stderr}");
}

#[test]
fn end_to_end_ignores_dpr_with_absolute_pattern_and_reports_info() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_repo");
    let expected_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_expected");
    let temp_root = temp_dir("fixdpr_e2e_ignore_dpr_");
    copy_dir(&fixture_root, &temp_root);

    let ignored_dpr = temp_root.join("app4").join("App4.dpr");
    let new_dependency = temp_root.join("common").join("NewUnit.pas");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("add-dependency")
        .current_dir(&repo_root)
        .arg("--search-path")
        .arg(&temp_root)
        .arg("--new-dependency")
        .arg(&new_dependency)
        .arg("--ignore-path")
        .arg(temp_root.join("ignored"))
        .arg("--ignore-dpr")
        .arg(&ignored_dpr)
        .arg("--show-infos")
        .output()
        .expect("run fixdpr");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Infos: 1"), "{stdout}");
    assert!(stdout.contains("Infos list:"), "{stdout}");
    assert!(stdout.contains("dpr ignored: 1"), "{stdout}");

    let app4_actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app4").join("App4.dpr")).expect("read app4 actual"),
    );
    let app4_expected = normalize_newlines(
        fs::read_to_string(fixture_root.join("app4").join("App4.dpr")).expect("read app4 expected"),
    );
    assert_eq!(app4_actual, app4_expected, "app4 should be ignored");

    let app1_actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app1").join("App1.dpr")).expect("read app1 actual"),
    );
    let app1_expected = normalize_newlines(
        fs::read_to_string(expected_root.join("app1").join("App1.dpr"))
            .expect("read app1 expected"),
    );
    assert_eq!(app1_actual, app1_expected, "app1 should still be updated");
}

#[test]
fn end_to_end_relative_ignore_pattern_from_repo_root_does_not_match_temp_repo() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_repo");
    let expected_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_expected");
    let temp_root = temp_dir("fixdpr_e2e_ignore_rel_repo_root_");
    copy_dir(&fixture_root, &temp_root);

    let new_dependency = temp_root.join("common").join("NewUnit.pas");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("add-dependency")
        .current_dir(&repo_root)
        .arg("--search-path")
        .arg(&temp_root)
        .arg("--new-dependency")
        .arg(&new_dependency)
        .arg("--ignore-path")
        .arg(temp_root.join("ignored"))
        .arg("--ignore-dpr")
        .arg("app4/*.dpr")
        .arg("--show-infos")
        .output()
        .expect("run fixdpr");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Infos: 0"), "{stdout}");
    assert!(stdout.contains("dpr ignored: 0"), "{stdout}");

    let app4_actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app4").join("App4.dpr")).expect("read app4 actual"),
    );
    let app4_expected = normalize_newlines(
        fs::read_to_string(expected_root.join("app4").join("App4.dpr"))
            .expect("read app4 expected"),
    );
    assert_eq!(app4_actual, app4_expected, "app4 should not be ignored");
}

#[test]
fn end_to_end_relative_ignore_pattern_from_search_root_matches() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_repo");
    let expected_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_expected");
    let temp_root = temp_dir("fixdpr_e2e_ignore_rel_search_root_");
    copy_dir(&fixture_root, &temp_root);

    let new_dependency = temp_root.join("common").join("NewUnit.pas");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("add-dependency")
        .current_dir(&temp_root)
        .arg("--search-path")
        .arg(&temp_root)
        .arg("--new-dependency")
        .arg(&new_dependency)
        .arg("--ignore-path")
        .arg("ignored")
        .arg("--ignore-dpr")
        .arg("app4/*.dpr")
        .arg("--show-infos")
        .output()
        .expect("run fixdpr");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Infos: 1"), "{stdout}");
    assert!(stdout.contains("dpr ignored: 1"), "{stdout}");

    let app4_actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app4").join("App4.dpr")).expect("read app4 actual"),
    );
    let app4_expected = normalize_newlines(
        fs::read_to_string(fixture_root.join("app4").join("App4.dpr")).expect("read app4 expected"),
    );
    assert_eq!(app4_actual, app4_expected, "app4 should be ignored");

    let app1_actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app1").join("App1.dpr")).expect("read app1 actual"),
    );
    let app1_expected = normalize_newlines(
        fs::read_to_string(expected_root.join("app1").join("App1.dpr"))
            .expect("read app1 expected"),
    );
    assert_eq!(app1_actual, app1_expected, "app1 should still be updated");
}

#[test]
fn end_to_end_delphi_path_enables_transitive_external_resolution() {
    let without_root = temp_dir("fixdpr_e2e_delphi_path_without_");
    let without_project = without_root.join("project");
    let without_delphi = without_root.join("delphi");
    create_delphi_path_fixture(&without_project, &without_delphi);

    let without_output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("add-dependency")
        .arg("--search-path")
        .arg(&without_project)
        .arg("--new-dependency")
        .arg(without_delphi.join("NewUnit.pas"))
        .output()
        .expect("run fixdpr without delphi path");

    assert!(
        without_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&without_output.stdout),
        String::from_utf8_lossy(&without_output.stderr)
    );

    let without_dpr = normalize_newlines(
        fs::read_to_string(without_project.join("App.dpr")).expect("read dpr without delphi path"),
    );
    assert!(
        !without_dpr.contains("NewUnit in "),
        "dpr should stay unchanged without --delphi-path:\n{without_dpr}"
    );

    let with_root = temp_dir("fixdpr_e2e_delphi_path_with_");
    let with_project = with_root.join("project");
    let with_delphi = with_root.join("delphi");
    create_delphi_path_fixture(&with_project, &with_delphi);

    let with_output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("add-dependency")
        .arg("--search-path")
        .arg(&with_project)
        .arg("--new-dependency")
        .arg(with_delphi.join("NewUnit.pas"))
        .arg("--delphi-path")
        .arg(&with_delphi)
        .output()
        .expect("run fixdpr with delphi path");

    assert!(
        with_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&with_output.stdout),
        String::from_utf8_lossy(&with_output.stderr)
    );

    let with_dpr =
        normalize_newlines(fs::read_to_string(with_project.join("App.dpr")).expect("read dpr"));
    assert!(
        with_dpr.contains("NewUnit in '..\\delphi\\NewUnit.pas'"),
        "dpr should include NewUnit via transitive external dependency:\n{with_dpr}"
    );
}

#[test]
fn end_to_end_delphi_version_reports_error_for_unknown_version() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_repo");
    let temp_root = temp_dir("fixdpr_e2e_delphi_version_unknown_");
    copy_dir(&fixture_root, &temp_root);

    let new_dependency = temp_root.join("common").join("NewUnit.pas");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("add-dependency")
        .arg("--search-path")
        .arg(&temp_root)
        .arg("--new-dependency")
        .arg(&new_dependency)
        .arg("--delphi-version")
        .arg("9999.9999")
        .output()
        .expect("run fixdpr with invalid delphi version");

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    #[cfg(windows)]
    assert!(
        stderr.contains("--delphi-version not found in registry"),
        "{stderr}"
    );
    #[cfg(not(windows))]
    assert!(
        stderr.contains("--delphi-version is only supported on Windows"),
        "{stderr}"
    );
}

#[test]
fn end_to_end_adds_introduced_dependencies_by_default() {
    let root = temp_dir("fixdpr_e2e_introduced_default_");
    let project_root = root.join("app");
    let shared_root = root.join("shared");
    create_introduced_dependency_fixture(&project_root, &shared_root);

    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("add-dependency")
        .arg("--search-path")
        .arg(&root)
        .arg("--new-dependency")
        .arg(shared_root.join("NewUnit.pas"))
        .output()
        .expect("run fixdpr with introduced dependencies enabled");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let dpr = normalize_newlines(
        fs::read_to_string(project_root.join("App.dpr")).expect("read updated dpr"),
    );
    assert!(
        dpr.contains("NewUnit in '..\\shared\\NewUnit.pas'"),
        "missing NewUnit entry:\n{dpr}"
    );
    assert!(
        dpr.contains("MidUnit in '..\\shared\\MidUnit.pas'"),
        "missing MidUnit entry:\n{dpr}"
    );
    assert!(
        dpr.contains("BaseUnit in '..\\shared\\BaseUnit.pas'"),
        "missing BaseUnit entry:\n{dpr}"
    );
}

#[test]
fn end_to_end_disable_introduced_dependencies_flag_restores_single_insert_behavior() {
    let root = temp_dir("fixdpr_e2e_introduced_disabled_");
    let project_root = root.join("app");
    let shared_root = root.join("shared");
    create_introduced_dependency_fixture(&project_root, &shared_root);

    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("add-dependency")
        .arg("--search-path")
        .arg(&root)
        .arg("--new-dependency")
        .arg(shared_root.join("NewUnit.pas"))
        .arg("--disable-introduced-dependencies")
        .output()
        .expect("run fixdpr with introduced dependencies disabled");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let dpr = normalize_newlines(
        fs::read_to_string(project_root.join("App.dpr")).expect("read updated dpr"),
    );
    assert!(
        dpr.contains("NewUnit in '..\\shared\\NewUnit.pas'"),
        "missing NewUnit entry:\n{dpr}"
    );
    assert!(
        !dpr.contains("MidUnit in '..\\shared\\MidUnit.pas'"),
        "MidUnit should not be inserted when disabled:\n{dpr}"
    );
    assert!(
        !dpr.contains("BaseUnit in '..\\shared\\BaseUnit.pas'"),
        "BaseUnit should not be inserted when disabled:\n{dpr}"
    );
}

#[test]
fn end_to_end_fix_dpr_repairs_missing_chain_for_target_file() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_repo");
    let temp_root = temp_dir("fixdpr_e2e_fix_dpr_");
    copy_dir(&fixture_root, &temp_root);

    let target_dpr = temp_root.join("app1").join("App1.dpr");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("fix-dpr")
        .arg("--search-path")
        .arg(&temp_root)
        .arg("--dpr-file")
        .arg(&target_dpr)
        .arg("--ignore-path")
        .arg(temp_root.join("ignored"))
        .output()
        .expect("run fixdpr fix-dpr mode");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("dpr scanned: 1"), "{stdout}");

    let app1 = normalize_newlines(
        fs::read_to_string(temp_root.join("app1").join("App1.dpr")).expect("read updated app1"),
    );
    assert!(app1.contains("UnitA in 'UnitA.pas'"), "{app1}");
    assert!(
        app1.contains("NewUnit in '..\\common\\NewUnit.pas'"),
        "{app1}"
    );

    let app2 = normalize_newlines(
        fs::read_to_string(temp_root.join("app2").join("App2.dpr")).expect("read app2"),
    );
    let app2_expected = normalize_newlines(
        fs::read_to_string(fixture_root.join("app2").join("App2.dpr")).expect("read app2 expected"),
    );
    assert_eq!(
        app2, app2_expected,
        "non-target dpr should remain unchanged"
    );
}

#[test]
fn end_to_end_fix_dpr_rejects_target_ignored_by_pattern() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_repo");
    let temp_root = temp_dir("fixdpr_e2e_fix_dpr_ignore_");
    copy_dir(&fixture_root, &temp_root);

    let target_dpr = temp_root.join("app1").join("App1.dpr");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("fix-dpr")
        .arg("--search-path")
        .arg(&temp_root)
        .arg("--dpr-file")
        .arg(&target_dpr)
        .arg("--ignore-dpr")
        .arg(&target_dpr)
        .output()
        .expect("run fixdpr fix-dpr mode with ignored target");

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--dpr-file is ignored by --ignore-dpr"),
        "{stderr}"
    );
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create dst");
    for entry in fs::read_dir(src).expect("read dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_dir(&path, &target);
        } else {
            fs::copy(&path, &target).expect("copy file");
        }
    }
}

fn temp_dir(prefix: &str) -> PathBuf {
    let mut root = env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    root.push(format!("{prefix}{nanos}"));
    fs::create_dir_all(&root).expect("create temp dir");
    root
}

fn normalize_newlines(contents: String) -> String {
    contents.replace("\r\n", "\n")
}

fn create_introduced_dependency_fixture(project_root: &Path, shared_root: &Path) {
    fs::create_dir_all(project_root).expect("create project root");
    fs::create_dir_all(shared_root).expect("create shared root");

    fs::write(
        project_root.join("App.dpr"),
        "program App;\nuses\n  UnitA in 'UnitA.pas';\nbegin\nend.\n",
    )
    .expect("write App.dpr");
    fs::write(
        project_root.join("UnitA.pas"),
        "unit UnitA;\ninterface\nuses NewUnit;\nimplementation\nend.\n",
    )
    .expect("write UnitA.pas");
    fs::write(
        shared_root.join("NewUnit.pas"),
        "unit NewUnit;\ninterface\nuses MidUnit;\nimplementation\nend.\n",
    )
    .expect("write NewUnit.pas");
    fs::write(
        shared_root.join("MidUnit.pas"),
        "unit MidUnit;\ninterface\nuses BaseUnit;\nimplementation\nend.\n",
    )
    .expect("write MidUnit.pas");
    fs::write(
        shared_root.join("BaseUnit.pas"),
        "unit BaseUnit;\ninterface\nimplementation\nend.\n",
    )
    .expect("write BaseUnit.pas");
}

fn create_delphi_path_fixture(project_root: &Path, delphi_root: &Path) {
    fs::create_dir_all(project_root).expect("create project root");
    fs::create_dir_all(delphi_root).expect("create delphi root");

    fs::write(
        project_root.join("App.dpr"),
        "program App;\nuses\n  UnitA in 'UnitA.pas';\nbegin\nend.\n",
    )
    .expect("write App.dpr");
    fs::write(
        project_root.join("UnitA.pas"),
        "unit UnitA;\ninterface\nuses ExtMid;\nimplementation\nend.\n",
    )
    .expect("write UnitA.pas");

    fs::write(
        delphi_root.join("ExtMid.pas"),
        "unit ExtMid;\ninterface\nuses NewUnit;\nimplementation\nend.\n",
    )
    .expect("write ExtMid.pas");
    fs::write(
        delphi_root.join("NewUnit.pas"),
        "unit NewUnit;\ninterface\nimplementation\nend.\n",
    )
    .expect("write NewUnit.pas");
}
