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
fn end_to_end_add_dependency_uses_conditional_dependents_by_default() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("assume_off_repo");
    let expected_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("assume_off_expected_default");
    let temp_root = temp_dir("fixdpr_e2e_assume_off_default_");
    copy_dir(&fixture_root, &temp_root);

    let new_dependency = temp_root.join("shared").join("NewUnit.pas");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("add-dependency")
        .arg("--search-path")
        .arg(&temp_root)
        .arg(&new_dependency)
        .output()
        .expect("run fixdpr add-dependency default conditional lookup");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app").join("App.dpr")).expect("read actual dpr"),
    );
    let expected = normalize_newlines(
        fs::read_to_string(expected_root.join("app").join("App.dpr")).expect("read expected dpr"),
    );
    assert_eq!(
        actual, expected,
        "conditional dependency should be inserted"
    );
}

#[test]
fn end_to_end_add_dependency_assume_off_skips_conditional_dependents() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("assume_off_repo");
    let temp_root = temp_dir("fixdpr_e2e_assume_off_disabled_");
    copy_dir(&fixture_root, &temp_root);

    let new_dependency = temp_root.join("shared").join("NewUnit.pas");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("add-dependency")
        .arg("--search-path")
        .arg(&temp_root)
        .arg("--assume-off")
        .arg("DEBUG")
        .arg(&new_dependency)
        .output()
        .expect("run fixdpr add-dependency with assume-off");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app").join("App.dpr")).expect("read actual dpr"),
    );
    let expected = normalize_newlines(
        fs::read_to_string(fixture_root.join("app").join("App.dpr")).expect("read expected dpr"),
    );
    assert_eq!(
        actual, expected,
        "assumed-off branch should not trigger insertion"
    );
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
fn end_to_end_delete_dependency_removes_orphaned_dependencies() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root.join("tests").join("fixtures").join("delete_repo");
    let expected_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("delete_expected");
    let temp_root = temp_dir("fixdpr_e2e_delete_");
    copy_dir(&fixture_root, &temp_root);

    let old_dependency = temp_root.join("common").join("OldUnit.pas");
    let target_dpr = temp_root.join("app").join("App.dpr");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("delete-dependency")
        .arg("--search-path")
        .arg(&temp_root)
        .arg("--target-dpr")
        .arg(&target_dpr)
        .arg(&old_dependency)
        .output()
        .expect("run fixdpr");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app").join("App.dpr")).expect("read app actual"),
    );
    let expected = normalize_newlines(
        fs::read_to_string(expected_root.join("app").join("App.dpr")).expect("read app expected"),
    );
    assert_eq!(
        actual, expected,
        "delete-dependency should remove OldUnit and LeafOnly only"
    );
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
fn end_to_end_fix_dpr_delphi_path_enables_transitive_external_resolution() {
    let without_root = temp_dir("fixdpr_e2e_fix_dpr_delphi_path_without_");
    let without_project = without_root.join("project");
    let without_delphi = without_root.join("delphi");
    create_delphi_path_fixture(&without_project, &without_delphi);

    let without_target = without_project.join("App.dpr");
    let without_output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("fix-dpr")
        .arg("--search-path")
        .arg(&without_project)
        .arg(&without_target)
        .output()
        .expect("run fixdpr fix-dpr without delphi path");

    assert!(
        without_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&without_output.stdout),
        String::from_utf8_lossy(&without_output.stderr)
    );

    let without_dpr = normalize_newlines(
        fs::read_to_string(&without_target).expect("read dpr without delphi path"),
    );
    assert!(
        !without_dpr.contains("ExtMid in "),
        "dpr should stay unchanged without --delphi-path:\n{without_dpr}"
    );
    assert!(
        !without_dpr.contains("NewUnit in "),
        "dpr should stay unchanged without --delphi-path:\n{without_dpr}"
    );

    let with_root = temp_dir("fixdpr_e2e_fix_dpr_delphi_path_with_");
    let with_project = with_root.join("project");
    let with_delphi = with_root.join("delphi");
    create_delphi_path_fixture(&with_project, &with_delphi);

    let with_target = with_project.join("App.dpr");
    let with_output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("fix-dpr")
        .arg("--search-path")
        .arg(&with_project)
        .arg(&with_target)
        .arg("--delphi-path")
        .arg(&with_delphi)
        .output()
        .expect("run fixdpr fix-dpr with delphi path");

    assert!(
        with_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&with_output.stdout),
        String::from_utf8_lossy(&with_output.stderr)
    );

    let with_dpr = normalize_newlines(fs::read_to_string(&with_target).expect("read dpr"));
    assert!(
        with_dpr.contains("ExtMid in '..\\delphi\\ExtMid.pas'"),
        "dpr should include ExtMid via external dependency:\n{with_dpr}"
    );
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
fn end_to_end_fix_dpr_delphi_version_reports_error_for_unknown_version() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_repo");
    let temp_root = temp_dir("fixdpr_e2e_fix_dpr_delphi_version_unknown_");
    copy_dir(&fixture_root, &temp_root);

    let target_dpr = temp_root.join("app1").join("App1.dpr");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("fix-dpr")
        .arg("--search-path")
        .arg(&temp_root)
        .arg(&target_dpr)
        .arg("--delphi-version")
        .arg("9999.9999")
        .output()
        .expect("run fixdpr fix-dpr with invalid delphi version");

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
fn end_to_end_add_dependency_can_run_fix_dpr_on_updated_files() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_repo");
    let expected_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_expected");
    let temp_root = temp_dir("fixdpr_e2e_add_then_fix_");
    copy_dir(&fixture_root, &temp_root);

    let new_dependency = temp_root.join("common").join("NewUnit.pas");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("add-dependency")
        .arg("--search-path")
        .arg(&temp_root)
        .arg(&new_dependency)
        .arg("--ignore-path")
        .arg(temp_root.join("ignored"))
        .arg("--fix-updated-dprs")
        .output()
        .expect("run fixdpr add-dependency with follow-up fix mode");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Running fix-dpr pass on updated dpr files"),
        "{stdout}"
    );

    let app1 = normalize_newlines(
        fs::read_to_string(temp_root.join("app1").join("App1.dpr")).expect("read app1"),
    );
    assert!(
        app1.contains("UnitA in 'UnitA.pas'"),
        "follow-up fix pass should add UnitA to app1:\n{app1}"
    );
    assert!(
        app1.contains("NewUnit in '..\\common\\NewUnit.pas'"),
        "app1 should still contain new dependency:\n{app1}"
    );

    let app2_actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app2").join("App2.dpr")).expect("read app2 actual"),
    );
    let app2_expected = normalize_newlines(
        fs::read_to_string(expected_root.join("app2").join("App2.dpr"))
            .expect("read app2 expected"),
    );
    assert_eq!(app2_actual, app2_expected, "app2 should remain unchanged");
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
fn end_to_end_fix_dpr_rejects_ignore_dpr_flag() {
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
        .arg(&target_dpr)
        .arg("--ignore-dpr")
        .arg(&target_dpr)
        .output()
        .expect("run fixdpr fix-dpr mode with unsupported flag");

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument '--ignore-dpr'"),
        "{stderr}"
    );
}

#[test]
fn end_to_end_list_conditionals_reports_simple_and_complex_buckets() {
    let root = temp_dir("fixdpr_e2e_list_conditionals_");
    create_list_conditionals_fixture(&root);

    let target_dpr = root.join("App.dpr");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("list-conditionals")
        .arg("--search-path")
        .arg(&root)
        .arg(&target_dpr)
        .output()
        .expect("run fixdpr list-conditionals");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Mode: list-conditionals"), "{stdout}");
    assert!(stdout.contains("Unconditional units"), "{stdout}");
    assert!(stdout.contains("  AlwaysUnit"), "{stdout}");
    assert!(stdout.contains("  SharedAlways"), "{stdout}");
    assert!(
        stdout.contains("Units only if DEBUG is defined"),
        "{stdout}"
    );
    assert!(stdout.contains("  DebugRoot"), "{stdout}");
    assert!(stdout.contains("  DebugChild"), "{stdout}");
    assert!(
        stdout.contains("Units only if TRACE is not defined"),
        "{stdout}"
    );
    assert!(stdout.contains("  TraceOffRoot"), "{stdout}");
    assert!(stdout.contains("  TraceChild"), "{stdout}");
    assert!(stdout.contains("  HiddenTraceOff"), "{stdout}");
    assert!(stdout.contains("Units with complex conditions"), "{stdout}");
    assert!(
        stdout.contains("FeatureUnit: DEBUG AND FEATURE"),
        "{stdout}"
    );
    assert!(
        stdout.contains("ComplexRoot: OUTER AND NOT INNER"),
        "{stdout}"
    );
    assert!(
        stdout.contains("ComplexChild: OUTER AND NOT INNER"),
        "{stdout}"
    );
}

#[test]
fn end_to_end_list_conditionals_uses_delphi_fallback_resolution() {
    let root = temp_dir("fixdpr_e2e_list_conditionals_delphi_");
    let project_root = root.join("project");
    let delphi_root = root.join("delphi");
    create_list_conditionals_delphi_fixture(&project_root, &delphi_root);

    let target_dpr = project_root.join("App.dpr");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("list-conditionals")
        .arg("--search-path")
        .arg(&project_root)
        .arg(&target_dpr)
        .arg("--delphi-path")
        .arg(&delphi_root)
        .output()
        .expect("run fixdpr list-conditionals with delphi path");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Units only if DEBUG is defined"),
        "{stdout}"
    );
    assert!(stdout.contains("  ExtRoot"), "{stdout}");
    assert!(stdout.contains("  ExtChild"), "{stdout}");
}

#[test]
fn end_to_end_list_conditionals_warns_on_unsupported_directives() {
    let root = temp_dir("fixdpr_e2e_list_conditionals_unsupported_");
    create_list_conditionals_unsupported_fixture(&root);

    let target_dpr = root.join("App.dpr");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("list-conditionals")
        .arg("--search-path")
        .arg(&root)
        .arg(&target_dpr)
        .arg("--show-warnings")
        .output()
        .expect("run fixdpr list-conditionals with warnings");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Warnings: 1"), "{stdout}");
    assert!(
        stdout.contains("unsupported compiler directive DEFINE"),
        "{stdout}"
    );
    assert!(stdout.contains("LeafUnit: UNKNOWN(DEFINE)"), "{stdout}");
}

#[test]
fn end_to_end_list_conditionals_supports_if_elseif_and_ifopt() {
    let root = temp_dir("fixdpr_e2e_list_conditionals_if_");
    create_list_conditionals_if_fixture(&root);

    let target_dpr = root.join("App.dpr");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("list-conditionals")
        .arg("--search-path")
        .arg(&root)
        .arg(&target_dpr)
        .arg("--show-warnings")
        .output()
        .expect("run fixdpr list-conditionals with if/elseif/ifopt");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Warnings: 0"), "{stdout}");
    assert!(
        stdout.contains("Units only if DEBUG is defined"),
        "{stdout}"
    );
    assert!(stdout.contains("  IfDebug"), "{stdout}");
    assert!(stdout.contains("SharedAlways"), "{stdout}");
    assert!(stdout.contains("IfTrace: TRACE AND NOT DEBUG"), "{stdout}");
    assert!(
        stdout.contains("IfFallback: NOT DEBUG AND NOT TRACE"),
        "{stdout}"
    );
    assert!(stdout.contains("OptOn: IFOPT(N+)"), "{stdout}");
    assert!(stdout.contains("OptOff: NOT IFOPT(N+)"), "{stdout}");
}

#[test]
fn end_to_end_list_conditionals_rejects_assume_off_flag() {
    let root = temp_dir("fixdpr_e2e_list_conditionals_assume_off_");
    create_list_conditionals_fixture(&root);

    let target_dpr = root.join("App.dpr");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("list-conditionals")
        .arg("--search-path")
        .arg(&root)
        .arg(&target_dpr)
        .arg("--assume-off")
        .arg("DEBUG")
        .output()
        .expect("run fixdpr list-conditionals with unsupported assume-off flag");

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument '--assume-off'"),
        "{stderr}"
    );
}

#[test]
fn end_to_end_list_conditionals_keeps_unsupported_if_branch_local() {
    let root = temp_dir("fixdpr_e2e_list_conditionals_if_fallback_");
    create_list_conditionals_if_fallback_fixture(&root);

    let target_dpr = root.join("App.dpr");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("list-conditionals")
        .arg("--search-path")
        .arg(&root)
        .arg(&target_dpr)
        .arg("--show-warnings")
        .output()
        .expect("run fixdpr list-conditionals with unsupported if expression");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Warnings: 1"), "{stdout}");
    assert!(
        stdout.contains("unsupported IF expression RTLVersion >= 14"),
        "{stdout}"
    );
    assert!(!stdout.contains("unmatched ELSE"), "{stdout}");
    assert!(!stdout.contains("unmatched ENDIF"), "{stdout}");
    assert!(
        stdout.contains("Foo: UNKNOWN(IF: RTLVERSION >= 14)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Bar: NOT UNKNOWN(IF: RTLVERSION >= 14)"),
        "{stdout}"
    );
}

#[test]
fn end_to_end_insert_dependency_targets_path_and_creates_uses_section() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root.join("tests").join("fixtures").join("insert_repo");
    let expected_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("insert_expected_target_path");
    let temp_root = temp_dir("fixdpr_e2e_insert_target_path_");
    copy_dir(&fixture_root, &temp_root);

    let new_dependency = temp_root.join("common").join("NewUnit.pas");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("insert-dependency")
        .arg("--search-path")
        .arg(&temp_root)
        .arg("--target-path")
        .arg(temp_root.join("apps"))
        .arg(&new_dependency)
        .output()
        .expect("run fixdpr insert-dependency with target path");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let expected_files = [
        PathBuf::from("apps").join("NoUses").join("AppNoUses.dpr"),
        PathBuf::from("apps").join("HasUses").join("AppHasUses.dpr"),
        PathBuf::from("other").join("ExplicitOnly.dpr"),
    ];
    for rel_path in expected_files {
        let actual = normalize_newlines(
            fs::read_to_string(temp_root.join(&rel_path))
                .unwrap_or_else(|_| panic!("missing actual file: {}", rel_path.display())),
        );
        let expected = normalize_newlines(
            fs::read_to_string(expected_root.join(&rel_path))
                .unwrap_or_else(|_| panic!("missing expected file: {}", rel_path.display())),
        );
        assert_eq!(actual, expected, "mismatch for {}", rel_path.display());
    }
}

#[test]
fn end_to_end_insert_dependency_targets_explicit_dpr_file() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root.join("tests").join("fixtures").join("insert_repo");
    let temp_root = temp_dir("fixdpr_e2e_insert_target_dpr_");
    copy_dir(&fixture_root, &temp_root);

    let target_dpr = temp_root.join("other").join("ExplicitOnly.dpr");
    let new_dependency = temp_root.join("common").join("NewUnit.pas");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("insert-dependency")
        .arg("--search-path")
        .arg(&temp_root)
        .arg("--target-dpr")
        .arg(&target_dpr)
        .arg("--disable-introduced-dependencies")
        .arg(&new_dependency)
        .output()
        .expect("run fixdpr insert-dependency with target dpr");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let explicit = normalize_newlines(fs::read_to_string(&target_dpr).expect("read target dpr"));
    assert!(
        explicit.contains("uses\n  NewUnit in '..\\common\\NewUnit.pas';"),
        "{explicit}"
    );

    let untouched = normalize_newlines(
        fs::read_to_string(temp_root.join("apps").join("NoUses").join("AppNoUses.dpr"))
            .expect("read untouched dpr"),
    );
    assert_eq!(untouched, "program AppNoUses;\nbegin\nend.\n");
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

fn create_list_conditionals_fixture(root: &Path) {
    fs::create_dir_all(root).expect("create root");

    write_file(
        root,
        "App.dpr",
        "program App;\nuses\n  AlwaysUnit in 'AlwaysUnit.pas',\n  {$IFDEF DEBUG} DebugRoot in 'DebugRoot.pas', {$ENDIF}\n  {$IFNDEF TRACE} TraceOffRoot in 'TraceOffRoot.pas', {$ENDIF}\n  {$IFDEF OUTER}{$IFNDEF INNER} ComplexRoot in 'ComplexRoot.pas', {$ENDIF}{$ENDIF};\nbegin\nend.\n",
    );
    write_file(
        root,
        "AlwaysUnit.pas",
        "unit AlwaysUnit;\ninterface\nuses SharedAlways;\nimplementation\nend.\n",
    );
    write_file(
        root,
        "SharedAlways.pas",
        "unit SharedAlways;\ninterface\nimplementation\nend.\n",
    );
    write_file(
        root,
        "DebugRoot.pas",
        "unit DebugRoot;\ninterface\nuses DebugChild, {$IFDEF FEATURE} FeatureUnit, {$ENDIF};\nimplementation\nend.\n",
    );
    write_file(
        root,
        "DebugChild.pas",
        "unit DebugChild;\ninterface\nimplementation\nend.\n",
    );
    write_file(
        root,
        "FeatureUnit.pas",
        "unit FeatureUnit;\ninterface\nimplementation\nend.\n",
    );
    write_file(
        root,
        "TraceOffRoot.pas",
        "unit TraceOffRoot;\ninterface\nuses {$I TraceUses.inc} TraceChild;\nimplementation\nend.\n",
    );
    write_file(
        root,
        "TraceUses.inc",
        "{$IFNDEF TRACE} HiddenTraceOff, {$ENDIF}",
    );
    write_file(
        root,
        "TraceChild.pas",
        "unit TraceChild;\ninterface\nimplementation\nend.\n",
    );
    write_file(
        root,
        "HiddenTraceOff.pas",
        "unit HiddenTraceOff;\ninterface\nimplementation\nend.\n",
    );
    write_file(
        root,
        "ComplexRoot.pas",
        "unit ComplexRoot;\ninterface\nuses ComplexChild;\nimplementation\nend.\n",
    );
    write_file(
        root,
        "ComplexChild.pas",
        "unit ComplexChild;\ninterface\nuses AlwaysUnit;\nimplementation\nend.\n",
    );
}

fn create_list_conditionals_delphi_fixture(project_root: &Path, delphi_root: &Path) {
    fs::create_dir_all(project_root).expect("create project root");
    fs::create_dir_all(delphi_root).expect("create delphi root");

    write_file(
        project_root,
        "App.dpr",
        "program App;\nuses\n  LocalUnit in 'LocalUnit.pas',\n  {$IFDEF DEBUG} ExtRoot, {$ENDIF}\n  AlwaysUnit in 'AlwaysUnit.pas';\nbegin\nend.\n",
    );
    write_file(
        project_root,
        "LocalUnit.pas",
        "unit LocalUnit;\ninterface\nimplementation\nend.\n",
    );
    write_file(
        project_root,
        "AlwaysUnit.pas",
        "unit AlwaysUnit;\ninterface\nimplementation\nend.\n",
    );
    write_file(
        delphi_root,
        "ExtRoot.pas",
        "unit ExtRoot;\ninterface\nuses ExtChild;\nimplementation\nend.\n",
    );
    write_file(
        delphi_root,
        "ExtChild.pas",
        "unit ExtChild;\ninterface\nimplementation\nend.\n",
    );
}

fn create_list_conditionals_unsupported_fixture(root: &Path) {
    fs::create_dir_all(root).expect("create root");

    write_file(
        root,
        "App.dpr",
        "program App;\nuses\n  RootUnit in 'RootUnit.pas';\nbegin\nend.\n",
    );
    write_file(
        root,
        "RootUnit.pas",
        "unit RootUnit;\ninterface\nuses FlaggedUnit in 'FlaggedUnit.pas';\nimplementation\nend.\n",
    );
    write_file(
        root,
        "FlaggedUnit.pas",
        "unit FlaggedUnit;\ninterface\n{$DEFINE FEATURE}\nuses LeafUnit in 'LeafUnit.pas';\nimplementation\nend.\n",
    );
    write_file(
        root,
        "LeafUnit.pas",
        "unit LeafUnit;\ninterface\nimplementation\nend.\n",
    );
}

fn create_list_conditionals_if_fixture(root: &Path) {
    fs::create_dir_all(root).expect("create root");

    write_file(
        root,
        "App.dpr",
        "program App;\nuses\n  ConditionalRoot in 'ConditionalRoot.pas',\n  OptRoot in 'OptRoot.pas';\nbegin\nend.\n",
    );
    write_file(
        root,
        "ConditionalRoot.pas",
        "unit ConditionalRoot;\ninterface\nuses {$IF defined(DEBUG)} IfDebug in 'IfDebug.pas', {$ELSEIF defined(TRACE)} IfTrace in 'IfTrace.pas', {$ELSE} IfFallback in 'IfFallback.pas', {$ENDIF} SharedAlways in 'SharedAlways.pas';\nimplementation\nend.\n",
    );
    write_file(
        root,
        "OptRoot.pas",
        "unit OptRoot;\ninterface\nuses {$IFOPT N+} OptOn in 'OptOn.pas', {$ELSE} OptOff in 'OptOff.pas', {$ENDIF} SharedAlways in 'SharedAlways.pas';\nimplementation\nend.\n",
    );
    write_file(
        root,
        "IfDebug.pas",
        "unit IfDebug;\ninterface\nimplementation\nend.\n",
    );
    write_file(
        root,
        "IfTrace.pas",
        "unit IfTrace;\ninterface\nimplementation\nend.\n",
    );
    write_file(
        root,
        "IfFallback.pas",
        "unit IfFallback;\ninterface\nimplementation\nend.\n",
    );
    write_file(
        root,
        "OptOn.pas",
        "unit OptOn;\ninterface\nimplementation\nend.\n",
    );
    write_file(
        root,
        "OptOff.pas",
        "unit OptOff;\ninterface\nimplementation\nend.\n",
    );
    write_file(
        root,
        "SharedAlways.pas",
        "unit SharedAlways;\ninterface\nimplementation\nend.\n",
    );
}

fn create_list_conditionals_if_fallback_fixture(root: &Path) {
    fs::create_dir_all(root).expect("create root");

    write_file(
        root,
        "App.dpr",
        "program App;\nuses\n  RootUnit in 'RootUnit.pas';\nbegin\nend.\n",
    );
    write_file(
        root,
        "RootUnit.pas",
        "unit RootUnit;\ninterface\nuses {$IF RTLVersion >= 14} Foo in 'Foo.pas', {$ELSE} Bar in 'Bar.pas' {$ENDIF};\nimplementation\nend.\n",
    );
    write_file(
        root,
        "Foo.pas",
        "unit Foo;\ninterface\nimplementation\nend.\n",
    );
    write_file(
        root,
        "Bar.pas",
        "unit Bar;\ninterface\nimplementation\nend.\n",
    );
}

fn write_file(root: &Path, name: &str, contents: &str) {
    let path = root.join(name);
    fs::write(path, contents).expect("write file");
}
