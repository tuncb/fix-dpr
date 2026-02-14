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
        .arg("--search-path")
        .arg(&temp_root)
        .arg("--new-dependency")
        .arg(&new_dependency)
        .arg("--ignore-paths")
        .arg("ignored")
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
fn end_to_end_search_path_glob_dedupes_overlapping_roots() {
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
        .arg("--search-path")
        .arg(temp_root.join("app*"))
        .arg("--search-path")
        .arg(temp_root.join("app1"))
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
fn end_to_end_unmatched_search_path_pattern_is_reported_as_warning() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_repo");
    let expected_root = repo_root
        .join("tests")
        .join("fixtures")
        .join("synthetic_expected");
    let temp_root = temp_dir("fixdpr_e2e_search_warn_");
    copy_dir(&fixture_root, &temp_root);

    let matched_root = temp_root.clone();
    let unmatched_pattern = temp_root.join("missing*");
    let new_dependency = temp_root.join("common").join("NewUnit.pas");
    let output = Command::new(env!("CARGO_BIN_EXE_fixdpr"))
        .arg("--search-path")
        .arg(&matched_root)
        .arg("--search-path")
        .arg(&unmatched_pattern)
        .arg("--new-dependency")
        .arg(&new_dependency)
        .arg("--show-warnings")
        .output()
        .expect("run fixdpr");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Warnings:"), "{stdout}");
    assert!(stdout.contains("Warnings list:"), "{stdout}");
    assert!(
        stdout.contains("--search-path pattern matched no directories"),
        "{stdout}"
    );
    assert!(
        stdout.contains(unmatched_pattern.to_string_lossy().as_ref()),
        "{stdout}"
    );

    let app1_actual = normalize_newlines(
        fs::read_to_string(temp_root.join("app1").join("App1.dpr")).expect("read app1 actual"),
    );
    let app1_expected = normalize_newlines(
        fs::read_to_string(expected_root.join("app1").join("App1.dpr"))
            .expect("read app1 expected"),
    );
    assert_eq!(app1_actual, app1_expected, "app1 should be updated");
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
        .current_dir(&repo_root)
        .arg("--search-path")
        .arg(&temp_root)
        .arg("--new-dependency")
        .arg(&new_dependency)
        .arg("--ignore-paths")
        .arg("ignored")
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
        .current_dir(&repo_root)
        .arg("--search-path")
        .arg(&temp_root)
        .arg("--new-dependency")
        .arg(&new_dependency)
        .arg("--ignore-paths")
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
        .current_dir(&temp_root)
        .arg("--search-path")
        .arg(&temp_root)
        .arg("--new-dependency")
        .arg(&new_dependency)
        .arg("--ignore-paths")
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
