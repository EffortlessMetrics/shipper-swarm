//! End-to-end tests for the full `shipper publish` flow.
//!
//! Tests cover single-crate and multi-crate publishes, state/receipt/events
//! verification, --dry-run-like behavior, --package scoping, custom --state-dir,
//! failed publishes, and re-running publish when everything is already published.

use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

use assert_cmd::Command;
use tempfile::tempdir;
use tiny_http::{Header, Response, Server, StatusCode};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(path, content).expect("write");
}

fn create_single_crate_workspace(root: &Path) {
    write_file(
        &root.join("Cargo.toml"),
        r#"
[workspace]
members = ["demo"]
resolver = "2"
"#,
    );

    write_file(
        &root.join("demo/Cargo.toml"),
        r#"
[package]
name = "demo"
version = "0.1.0"
edition = "2021"
"#,
    );
    write_file(&root.join("demo/src/lib.rs"), "pub fn demo() {}\n");
}

fn create_workspace(root: &Path) {
    write_file(
        &root.join("Cargo.toml"),
        r#"
[workspace]
members = ["core", "utils", "app"]
resolver = "2"
"#,
    );

    write_file(
        &root.join("core/Cargo.toml"),
        r#"
[package]
name = "core"
version = "0.1.0"
edition = "2021"
"#,
    );
    write_file(&root.join("core/src/lib.rs"), "pub fn core() {}\n");

    write_file(
        &root.join("utils/Cargo.toml"),
        r#"
[package]
name = "utils"
version = "0.1.0"
edition = "2021"

[dependencies]
core = { path = "../core" }
"#,
    );
    write_file(&root.join("utils/src/lib.rs"), "pub fn utils() {}\n");

    write_file(
        &root.join("app/Cargo.toml"),
        r#"
[package]
name = "app"
version = "0.1.0"
edition = "2021"

[dependencies]
core = { path = "../core" }
utils = { path = "../utils" }
"#,
    );
    write_file(&root.join("app/src/lib.rs"), "pub fn app() {}\n");
}

fn create_fake_cargo_proxy(bin_dir: &Path) {
    #[cfg(windows)]
    {
        fs::write(
            bin_dir.join("cargo.cmd"),
            "@echo off\r\nif \"%1\"==\"publish\" (\r\n  if not \"%SHIPPER_FAKE_PUBLISH_LOG%\"==\"\" echo %*>>\"%SHIPPER_FAKE_PUBLISH_LOG%\"\r\n  if not \"%SHIPPER_FAKE_PUBLISH_STDERR%\"==\"\" echo %SHIPPER_FAKE_PUBLISH_STDERR% 1>&2\r\n  if \"%SHIPPER_FAKE_PUBLISH_EXIT%\"==\"\" (exit /b 0) else (exit /b %SHIPPER_FAKE_PUBLISH_EXIT%)\r\n)\r\n\"%REAL_CARGO%\" %*\r\nexit /b %ERRORLEVEL%\r\n",
        )
        .expect("write fake cargo");
    }

    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;

        let path = bin_dir.join("cargo");
        fs::write(
            &path,
            "#!/usr/bin/env sh\nif [ \"$1\" = \"publish\" ]; then\n  if [ -n \"$SHIPPER_FAKE_PUBLISH_LOG\" ]; then\n    printf '%s\\n' \"$*\" >> \"$SHIPPER_FAKE_PUBLISH_LOG\"\n  fi\n  if [ -n \"$SHIPPER_FAKE_PUBLISH_STDERR\" ]; then\n    printf '%s\\n' \"$SHIPPER_FAKE_PUBLISH_STDERR\" >&2\n  fi\n  exit \"${SHIPPER_FAKE_PUBLISH_EXIT:-0}\"\nfi\n\"$REAL_CARGO\" \"$@\"\n",
        )
        .expect("write fake cargo");
        let mut perms = fs::metadata(&path).expect("meta").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).expect("chmod");
    }
}

fn path_sep() -> &'static str {
    if cfg!(windows) { ";" } else { ":" }
}

fn fake_cargo_bin_path(bin_dir: &Path) -> String {
    #[cfg(windows)]
    {
        bin_dir.join("cargo.cmd").display().to_string()
    }
    #[cfg(not(windows))]
    {
        bin_dir.join("cargo").display().to_string()
    }
}

struct TestRegistry {
    base_url: String,
    handle: thread::JoinHandle<()>,
}

impl TestRegistry {
    fn join(self) {
        self.handle.join().expect("join server");
    }
}

fn spawn_registry(statuses: Vec<u16>, expected_requests: usize) -> TestRegistry {
    let server = Server::http("127.0.0.1:0").expect("server");
    let base_url = format!("http://{}", server.server_addr());
    let handle = thread::spawn(move || {
        for idx in 0..expected_requests {
            let req = match server.recv_timeout(Duration::from_secs(30)) {
                Ok(Some(r)) => r,
                _ => break,
            };
            let status = statuses
                .get(idx)
                .copied()
                .or_else(|| statuses.last().copied())
                .unwrap_or(404);
            let resp = Response::from_string("{}")
                .with_status_code(StatusCode(status))
                .with_header(
                    Header::from_bytes("Content-Type", "application/json").expect("header"),
                );
            req.respond(resp).expect("respond");
        }
    });
    TestRegistry { base_url, handle }
}

fn shipper_cmd() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("shipper-cli"))
}

fn loopback_shipper_cmd() -> Command {
    let mut command = shipper_cmd();
    command.arg("--allow-loopback");
    command
}

fn assert_publish_early_error_json(
    output: &std::process::Output,
    expected_category: &str,
    state_dir: &Path,
) -> serde_json::Value {
    assert_eq!(output.status.code(), Some(1), "early publish failure exit");
    assert!(
        output.stdout.is_empty(),
        "JSON errors keep stdout empty; got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr utf8");
    assert_secret_absent_from_output_and_state(output, state_dir);
    let report: serde_json::Value =
        serde_json::from_str(&stderr).expect("stderr should be one JSON error envelope");
    assert_eq!(
        report["schema_version"].as_str(),
        Some("shipper.publish.error.v1")
    );
    assert_eq!(report["command"].as_str(), Some("publish"));
    assert_eq!(report["status"].as_str(), Some("failed"));
    assert_eq!(report["category"].as_str(), Some(expected_category));
    assert!(report["summary"].is_string());
    assert!(report["safe_to_rerun"]["value"].is_null());
    assert_eq!(
        report["safe_to_rerun"]["reason"].as_str(),
        Some("no completed receipt exists to prove a safe rerun")
    );
    assert!(report["next_action"]["command"].is_null());
    assert_eq!(
        report["next_action"]["requires_confirmation"].as_bool(),
        Some(false)
    );
    assert_eq!(report["evidence"], serde_json::json!([]));
    report
}

fn assert_secret_absent_from_output_and_state(output: &std::process::Output, state_dir: &Path) {
    assert_sentinel_absent_from_output_and_state(output, state_dir, "EARLY_ERROR_SECRET");
}

fn assert_sentinel_absent_from_output_and_state(
    output: &std::process::Output,
    state_dir: &Path,
    sentinel: &str,
) {
    for (surface, bytes) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
        let rendered = String::from_utf8_lossy(bytes);
        assert!(
            !rendered.contains(sentinel),
            "secret leaked in {surface}: {rendered}"
        );
    }
    if !state_dir.exists() {
        return;
    }
    let mut pending = vec![state_dir.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(&path).expect("read state directory") {
            let entry = entry.expect("state entry");
            let entry_path = entry.path();
            if entry_path.is_dir() {
                pending.push(entry_path);
                continue;
            }
            let Some(name) = entry_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if matches!(name, "state.json" | "events.jsonl" | "receipt.json") {
                let contents = fs::read(&entry_path).expect("read retained evidence");
                assert!(
                    !String::from_utf8_lossy(&contents).contains(sentinel),
                    "secret leaked in {}",
                    entry_path.display()
                );
            }
        }
    }
}

/// Build env vars needed for fake cargo, returning (new_path, real_cargo, fake_cargo).
fn fake_cargo_env(bin_dir: &Path) -> (String, String, String) {
    let old_path = std::env::var("PATH").unwrap_or_default();
    let mut new_path = bin_dir.display().to_string();
    if !old_path.is_empty() {
        new_path.push_str(path_sep());
        new_path.push_str(&old_path);
    }
    let real_cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let fake_cargo = fake_cargo_bin_path(bin_dir);
    (new_path, real_cargo, fake_cargo)
}

/// Set up a temp dir with fake cargo bin and return (bin_dir, new_path, real_cargo, fake_cargo).
fn setup_fake_cargo(td: &Path) -> (String, String, String) {
    let bin_dir = td.join("fake-bin");
    fs::create_dir_all(&bin_dir).expect("mkdir");
    create_fake_cargo_proxy(&bin_dir);
    fake_cargo_env(&bin_dir)
}

fn read_publish_log(path: &Path) -> Vec<String> {
    if !path.exists() {
        return Vec::new();
    }

    fs::read_to_string(path)
        .expect("read publish log")
        .lines()
        .map(str::to_owned)
        .collect()
}

fn command_package_state<'a>(packages: &'a [serde_json::Value], name: &str) -> &'a str {
    packages
        .iter()
        .find(|package| package["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("expected command package report for {name}"))["state"]
        .as_str()
        .unwrap_or_else(|| panic!("expected command package state for {name}"))
}

fn receipt_package_state<'a>(packages: &'a [serde_json::Value], name: &str) -> &'a str {
    packages
        .iter()
        .find(|package| package["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("expected receipt package for {name}"))["state"]["state"]
        .as_str()
        .unwrap_or_else(|| panic!("expected receipt package state for {name}"))
}

// ============================================================================
// Test 1: Single-crate publish success with state/receipt verification
// ============================================================================

#[test]
fn single_crate_publish_creates_state_and_receipt() {
    let td = tempdir().expect("tempdir");
    create_single_crate_workspace(td.path());
    let (new_path, real_cargo, fake_cargo) = setup_fake_cargo(td.path());

    // version-check 404 (not yet published), then readiness 200 (visible)
    let registry = spawn_registry(vec![404, 200], 2);

    let state_dir = td.path().join(".shipper");

    let output = loopback_shipper_cmd()
        .timeout(Duration::from_secs(20))
        .arg("--manifest-path")
        .arg(td.path().join("Cargo.toml"))
        .arg("--api-base")
        .arg(&registry.base_url)
        .arg("--allow-dirty")
        .arg("--verify-timeout")
        .arg("0ms")
        .arg("--verify-poll")
        .arg("0ms")
        .arg("--max-attempts")
        .arg("1")
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("publish")
        .env("PATH", &new_path)
        .env("REAL_CARGO", &real_cargo)
        .env("SHIPPER_CARGO_BIN", &fake_cargo)
        .env("SHIPPER_FAKE_PUBLISH_EXIT", "0")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).expect("publish stdout utf8");
    assert!(stdout.contains("Result: success"), "{stdout}");
    assert!(
        stdout.contains("Safe to rerun: yes — all packages reached a successful terminal state"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Next: publication is complete; retain the receipt and event evidence"),
        "{stdout}"
    );

    // Verify state.json exists and has plan_id
    let state_path = state_dir.join("state.json");
    assert!(state_path.exists(), "state.json should exist");
    let state_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("read state"))
            .expect("parse state");
    assert!(
        state_json.get("plan_id").is_some(),
        "state should have plan_id"
    );

    // Verify receipt.json exists and shows published
    let receipt_path = state_dir.join("receipt.json");
    assert!(receipt_path.exists(), "receipt.json should exist");
    let receipt: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&receipt_path).expect("read receipt"))
            .expect("parse receipt");
    assert!(
        receipt.get("plan_id").is_some(),
        "receipt should have plan_id"
    );

    let packages = receipt["packages"].as_array().expect("packages array");
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0]["name"].as_str(), Some("demo"));
    assert_eq!(
        packages[0]["state"]["state"].as_str(),
        Some("published"),
        "package should be marked published"
    );

    // Verify receipt has timing fields
    assert!(
        receipt.get("started_at").is_some(),
        "receipt should have started_at"
    );
    assert!(
        receipt.get("finished_at").is_some(),
        "receipt should have finished_at"
    );

    registry.join();
}

#[test]
fn publish_json_format_writes_command_envelope_to_stdout() {
    let td = tempdir().expect("tempdir");
    create_single_crate_workspace(td.path());
    let (new_path, real_cargo, fake_cargo) = setup_fake_cargo(td.path());

    let registry = spawn_registry(vec![404, 200], 2);
    let state_dir = td.path().join(".shipper");
    let state_dir_arg = Path::new(".shipper");

    let output = loopback_shipper_cmd()
        .timeout(Duration::from_secs(20))
        .arg("--manifest-path")
        .arg(td.path().join("Cargo.toml"))
        .arg("--api-base")
        .arg(&registry.base_url)
        .arg("--allow-dirty")
        .arg("--verify-timeout")
        .arg("0ms")
        .arg("--verify-poll")
        .arg("0ms")
        .arg("--max-attempts")
        .arg("1")
        .arg("--state-dir")
        .arg(state_dir_arg)
        .arg("--format")
        .arg("json")
        .arg("publish")
        .env("PATH", &new_path)
        .env("REAL_CARGO", &real_cargo)
        .env("SHIPPER_CARGO_BIN", &fake_cargo)
        .env("SHIPPER_FAKE_PUBLISH_EXIT", "0")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).expect("utf8");
    let report: serde_json::Value =
        serde_json::from_str(&stdout).expect("publish stdout should be command JSON");
    assert_eq!(
        report["schema_version"].as_str(),
        Some("shipper.publish.v1"),
        "publish JSON should carry a command-owned schema version"
    );
    assert_eq!(report["command"].as_str(), Some("publish"));
    assert_eq!(
        report["safe_to_rerun"].as_bool(),
        Some(true),
        "completed publish envelope should expose safe rerun posture"
    );
    assert_eq!(report["registry"].as_str(), Some("crates-io"));
    assert_eq!(
        report["state_dir"].as_str(),
        Some(state_dir_arg.to_string_lossy().as_ref()),
        "legacy state_dir must preserve the configured relative value"
    );
    assert_eq!(report["outcome"]["status"].as_str(), Some("success"));
    assert_eq!(
        report["outcome"]["safe_to_rerun"]["value"].as_bool(),
        Some(true)
    );
    assert_eq!(
        report["safe_to_rerun"], report["outcome"]["safe_to_rerun"]["value"],
        "legacy and typed rerun fields must share one computed value"
    );
    assert_eq!(
        report["outcome"]["next_action"]["kind"].as_str(),
        Some("none_complete")
    );
    let evidence = report["outcome"]["evidence"]
        .as_array()
        .expect("typed evidence array");
    let evidence = evidence
        .iter()
        .map(|path| path.as_str().expect("evidence path").to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        evidence,
        ["state.json", "events.jsonl", "receipt.json"]
            .map(|name| state_dir.join(name).to_string_lossy().into_owned()),
        "typed evidence must name the workspace-resolved state, events, and receipt files"
    );
    assert!(report["plan_id"].is_string(), "plan_id should be present");
    assert_eq!(report["published"].as_u64(), Some(1));
    assert_eq!(report["pending"].as_u64(), Some(0));
    assert_eq!(report["failed"].as_u64(), Some(0));
    assert_eq!(report["ambiguous"].as_u64(), Some(0));
    assert_eq!(report["uploaded"].as_u64(), Some(0));
    assert_eq!(report["skipped"].as_u64(), Some(0));
    assert_eq!(
        report["packages"][0]["name"].as_str(),
        Some("demo"),
        "command envelope should contain the published package"
    );
    assert_eq!(report["packages"][0]["state"].as_str(), Some("published"));
    assert_eq!(report["packages"][0]["attempts"].as_u64(), Some(1));
    assert_eq!(report["packages"][0]["reconciled"].as_bool(), Some(false));
    assert_eq!(
        report["artifacts"]["state"]["exists"].as_bool(),
        Some(true),
        "state artifact should exist"
    );
    for (artifact, file) in [
        ("state", "state.json"),
        ("events", "events.jsonl"),
        ("receipt", "receipt.json"),
    ] {
        assert_eq!(
            report["artifacts"][artifact]["path"].as_str(),
            Some(state_dir_arg.join(file).to_string_lossy().as_ref()),
            "legacy {artifact} artifact path must preserve the configured relative state directory"
        );
    }
    assert_eq!(
        report["artifacts"]["events"]["exists"].as_bool(),
        Some(true),
        "events artifact should exist"
    );
    assert_eq!(
        report["artifacts"]["receipt"]["exists"].as_bool(),
        Some(true),
        "receipt artifact should exist"
    );
    assert_eq!(
        report["artifacts"]["reconciliation"]["exists"].as_bool(),
        Some(false),
        "reconciliation artifact should be absent when no ambiguity occurred"
    );
    assert_eq!(
        report["receipt"]["receipt_version"].as_str(),
        Some("shipper.receipt.v2"),
        "receipt remains nested as its own evidence contract"
    );
    assert_eq!(
        report["receipt"]["packages"][0]["state"]["state"].as_str(),
        Some("published")
    );
    assert!(
        state_dir.join("receipt.json").exists(),
        "receipt artifact should still be written"
    );

    registry.join();
}

#[test]
fn publish_missing_and_invalid_manifests_emit_typed_json_errors() {
    let td = tempdir().expect("tempdir");
    let malformed = td.path().join("malformed.toml");
    write_file(&malformed, "[workspace\n");

    for (case, manifest) in [
        ("missing", td.path().join("missing.toml")),
        ("malformed", malformed),
    ] {
        let state_dir = td.path().join(format!("state-{case}"));
        let output = shipper_cmd()
            .timeout(Duration::from_secs(20))
            .arg("--manifest-path")
            .arg(manifest)
            .arg("--state-dir")
            .arg(&state_dir)
            .arg("--format")
            .arg("json")
            .arg("publish")
            .env("CARGO_REGISTRY_TOKEN", "EARLY_ERROR_SECRET")
            .assert()
            .get_output()
            .clone();
        let report = assert_publish_early_error_json(&output, "invalid_manifest", &state_dir);
        assert_eq!(
            report["next_action"]["kind"].as_str(),
            Some("resolve_blockers")
        );
        assert!(!state_dir.exists(), "plan errors must not create state");
    }
}

#[test]
fn publish_manifest_error_human_output_matches_typed_posture() {
    let td = tempdir().expect("tempdir");
    let state_dir = td.path().join("state-human");
    let output = shipper_cmd()
        .timeout(Duration::from_secs(20))
        .arg("--manifest-path")
        .arg(td.path().join("missing.toml"))
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("publish")
        .env("CARGO_REGISTRY_TOKEN", "EARLY_ERROR_SECRET")
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(output.stdout.is_empty());
    assert_secret_absent_from_output_and_state(&output, &state_dir);
    let stderr = String::from_utf8(output.stderr.clone()).expect("stderr utf8");
    assert!(stderr.contains("Error:"), "{stderr}");
    assert!(
        stderr.contains("Result: failed — the publish plan could not be built"),
        "{stderr}"
    );
    assert!(
        stderr.contains("Safe to rerun: unknown — no completed receipt exists"),
        "{stderr}"
    );
    assert!(
        stderr.contains("Next: resolve the reported failure"),
        "{stderr}"
    );
    assert!(
        stderr.contains("Evidence: none from a completed receipt"),
        "{stderr}"
    );
    assert!(!state_dir.exists(), "plan errors must not create state");
}

#[test]
fn publish_non_git_workspace_emits_typed_json_error() {
    let td = tempdir().expect("tempdir");
    create_single_crate_workspace(td.path());
    let state_dir = td.path().join("state-non-git");
    let output = loopback_shipper_cmd()
        .timeout(Duration::from_secs(20))
        .arg("--manifest-path")
        .arg(td.path().join("Cargo.toml"))
        .arg("--api-base")
        .arg("http://127.0.0.1:9")
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("--quiet")
        .arg("--format")
        .arg("json")
        .arg("publish")
        .env("CARGO_REGISTRY_TOKEN", "EARLY_ERROR_SECRET")
        .assert()
        .get_output()
        .clone();
    assert_publish_early_error_json(&output, "workspace_not_ready", &state_dir);
    assert!(!state_dir.join("receipt.json").exists());
}

#[test]
fn publish_unreachable_registry_emits_typed_json_error() {
    let td = tempdir().expect("tempdir");
    create_single_crate_workspace(td.path());
    let state_dir = td.path().join("state-unreachable");
    let output = loopback_shipper_cmd()
        .timeout(Duration::from_secs(20))
        .arg("--manifest-path")
        .arg(td.path().join("Cargo.toml"))
        .arg("--api-base")
        .arg("http://127.0.0.1:9")
        .arg("--allow-dirty")
        .arg("--skip-ownership-check")
        .arg("--verify-timeout")
        .arg("0ms")
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("--quiet")
        .arg("--format")
        .arg("json")
        .arg("publish")
        .env("CARGO_REGISTRY_TOKEN", "EARLY_ERROR_SECRET")
        .assert()
        .get_output()
        .clone();
    let report = assert_publish_early_error_json(&output, "registry_unreachable", &state_dir);
    assert_eq!(
        report["next_action"]["kind"].as_str(),
        Some("stop_and_investigate")
    );
    assert!(!state_dir.join("receipt.json").exists());
}

#[test]
fn publish_multi_registry_json_error_is_one_clean_envelope() {
    let td = tempdir().expect("tempdir");
    create_single_crate_workspace(td.path());
    let state_dir = td.path().join("state-multi-registry");
    let config = td.path().join("multi-registry.toml");
    write_file(
        &config,
        r#"
[[registries.registries]]
name = "alpha"
api_base = "http://127.0.0.1:9"
index_base = "http://127.0.0.1:9"

[[registries.registries]]
name = "beta"
api_base = "https://example.invalid"
index_base = "https://example.invalid"

[rehearsal]
registry = "alpha"
allow_loopback = true
"#,
    );
    let output = loopback_shipper_cmd()
        .timeout(Duration::from_secs(20))
        .arg("--manifest-path")
        .arg(td.path().join("Cargo.toml"))
        .arg("--config")
        .arg(&config)
        .arg("--registries")
        .arg("alpha,beta")
        .arg("--skip-ownership-check")
        .arg("--verify-timeout")
        .arg("0ms")
        .arg("--max-attempts")
        .arg("1")
        .arg("--base-delay")
        .arg("0ms")
        .arg("--max-delay")
        .arg("0ms")
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("--format")
        .arg("json")
        .arg("publish")
        .env("CARGO_REGISTRY_TOKEN", "EARLY_ERROR_SECRET")
        .assert()
        .get_output()
        .clone();
    let report = assert_publish_early_error_json(&output, "workspace_not_ready", &state_dir);
    assert_eq!(
        report["next_action"]["kind"].as_str(),
        Some("resolve_blockers")
    );
    assert!(!state_dir.join("receipt.json").exists());
}

#[test]
fn completed_partial_publish_keeps_completed_json_contract_and_exit_two() {
    let td = tempdir().expect("tempdir");
    create_single_crate_workspace(td.path());
    let state_dir = td.path().join(".shipper");
    let output = loopback_shipper_cmd()
        .timeout(Duration::from_secs(20))
        .arg("--manifest-path")
        .arg(td.path().join("Cargo.toml"))
        .arg("--api-base")
        .arg("http://127.0.0.1:9")
        .arg("--allow-dirty")
        .arg("--skip-ownership-check")
        .arg("--max-attempts")
        .arg("0")
        .arg("--quiet")
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("--format")
        .arg("json")
        .arg("publish")
        .env("CARGO_REGISTRY_TOKEN", "EARLY_ERROR_SECRET")
        .assert()
        .get_output()
        .clone();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty(), "completed JSON stays off stderr");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("publish JSON");
    assert_eq!(
        report["schema_version"].as_str(),
        Some("shipper.publish.v1")
    );
    assert_eq!(report["execution_result"].as_str(), Some("partial_failure"));
    assert_ne!(
        report["outcome"]["next_action"]["kind"].as_str(),
        Some("none_complete")
    );
    assert!(state_dir.join("state.json").exists());
    assert!(state_dir.join("events.jsonl").exists());
    assert!(state_dir.join("receipt.json").exists());
    assert_secret_absent_from_output_and_state(&output, &state_dir);
}

#[test]
fn multi_registry_later_success_does_not_mask_earlier_partial_result() {
    const SECRET: &str = "MULTI_REGISTRY_OUTCOME_SECRET";

    let td = tempdir().expect("tempdir");
    create_single_crate_workspace(td.path());
    let (new_path, real_cargo, fake_cargo) = setup_fake_cargo(td.path());
    let alpha = spawn_registry(vec![404], 1);
    let beta = spawn_registry(vec![200], 1);
    let config = td.path().join("multi-registry.toml");
    write_file(
        &config,
        &format!(
            r#"
schema_version = "shipper.config.v1"

[[registries.registries]]
name = "alpha"
api_base = "{alpha}"
index_base = "{alpha}"

[[registries.registries]]
name = "beta"
api_base = "{beta}"
index_base = "{beta}"
"#,
            alpha = alpha.base_url,
            beta = beta.base_url,
        ),
    );

    let state_dir = td.path().join("state-multi-registry-outcome");
    let output = loopback_shipper_cmd()
        .timeout(Duration::from_secs(20))
        .arg("--manifest-path")
        .arg(td.path().join("Cargo.toml"))
        .arg("--config")
        .arg(&config)
        .arg("--registries")
        .arg("alpha,beta")
        .arg("--allow-dirty")
        .arg("--skip-ownership-check")
        .arg("--verify-timeout")
        .arg("0ms")
        .arg("--verify-poll")
        .arg("0ms")
        .arg("--max-attempts")
        .arg("0")
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("publish")
        .env("PATH", &new_path)
        .env("REAL_CARGO", &real_cargo)
        .env("SHIPPER_CARGO_BIN", &fake_cargo)
        .env("CARGO_REGISTRY_TOKEN", SECRET)
        .assert()
        .get_output()
        .clone();

    assert_eq!(
        output.status.code(),
        Some(2),
        "the later successful registry must not mask alpha's partial result"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let alpha_heading = stdout
        .find("Publishing to registry: alpha")
        .expect("alpha heading");
    let alpha_result = stdout
        .find("Result: partial failure")
        .expect("alpha partial result");
    let beta_heading = stdout
        .find("Publishing to registry: beta")
        .expect("beta heading");
    let beta_result = stdout
        .rfind("Result: success")
        .expect("beta success result");
    assert!(
        alpha_heading < alpha_result && alpha_result < beta_heading && beta_heading < beta_result,
        "registry headings and results must retain dispatcher order: {stdout}"
    );

    for (registry, expected_result, expected_state) in [
        ("alpha", "partial_failure", "pending"),
        ("beta", "success", "skipped"),
    ] {
        let registry_state = state_dir.join(registry);
        for artifact in ["state.json", "events.jsonl", "receipt.json"] {
            assert!(
                registry_state.join(artifact).exists(),
                "{registry} must retain {artifact}"
            );
        }

        let receipt: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(registry_state.join("receipt.json")).expect("read receipt"),
        )
        .expect("parse receipt");
        assert_eq!(
            receipt["execution_result"].as_str(),
            Some(expected_result),
            "{registry} receipt result"
        );
        assert_eq!(
            receipt["packages"][0]["state"]["state"].as_str(),
            Some(expected_state),
            "{registry} package state"
        );

        let events =
            fs::read_to_string(registry_state.join("events.jsonl")).expect("read registry events");
        assert!(events.contains(r#""type":"execution_finished""#));
        assert!(events.contains(expected_result));
    }

    assert_sentinel_absent_from_output_and_state(&output, &state_dir, SECRET);
    alpha.join();
    beta.join();
}

#[test]
fn publish_usage_error_keeps_clap_exit_two_without_execution_envelope() {
    let td = tempdir().expect("tempdir");
    let output = shipper_cmd()
        .timeout(Duration::from_secs(20))
        .current_dir(td.path())
        .arg("--definitely-invalid")
        .arg("publish")
        .assert()
        .get_output()
        .clone();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(stderr.contains("unexpected argument"), "{stderr}");
    assert!(stderr.contains("Usage:"), "{stderr}");
    assert!(!stderr.contains("shipper.publish.error.v1"), "{stderr}");
    assert!(!stderr.contains("Safe to rerun"), "{stderr}");
    assert!(!td.path().join(".shipper").exists());
}

// ============================================================================
// Test 2: Multi-crate workspace respects dependency ordering
// ============================================================================

#[test]
fn multi_crate_publish_respects_dependency_order() {
    let td = tempdir().expect("tempdir");
    create_workspace(td.path());
    let (new_path, real_cargo, fake_cargo) = setup_fake_cargo(td.path());

    // 3 crates × (version-check 404 + readiness 200) = 6 requests
    let registry = spawn_registry(vec![404, 200, 404, 200, 404, 200], 6);

    let state_dir = td.path().join(".shipper");

    let output = loopback_shipper_cmd()
        .arg("--manifest-path")
        .arg(td.path().join("Cargo.toml"))
        .arg("--api-base")
        .arg(&registry.base_url)
        .arg("--allow-dirty")
        .arg("--verify-timeout")
        .arg("0ms")
        .arg("--verify-poll")
        .arg("0ms")
        .arg("--max-attempts")
        .arg("1")
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("publish")
        .env("PATH", &new_path)
        .env("REAL_CARGO", &real_cargo)
        .env("SHIPPER_CARGO_BIN", &fake_cargo)
        .env("SHIPPER_FAKE_PUBLISH_EXIT", "0")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).expect("utf8");

    // All 3 packages should be published
    let receipt: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(state_dir.join("receipt.json")).expect("read receipt"),
    )
    .expect("parse");
    let packages = receipt["packages"].as_array().expect("packages");
    assert_eq!(packages.len(), 3, "all 3 packages should be in receipt");

    let published_count = packages
        .iter()
        .filter(|p| p["state"]["state"].as_str() == Some("published"))
        .count();
    assert_eq!(published_count, 3, "all 3 packages should be published");

    // Verify dependency order: core appears before utils, utils before app in stdout
    let core_pos = stdout.find("core@0.1.0").expect("core in output");
    let utils_pos = stdout.find("utils@0.1.0").expect("utils in output");
    let app_pos = stdout.find("app@0.1.0").expect("app in output");
    assert!(
        core_pos < utils_pos,
        "core should be published before utils"
    );
    assert!(utils_pos < app_pos, "utils should be published before app");

    registry.join();
}

// ============================================================================
// Test 3: Publish with --policy fast + preflight doesn't create state for plan
// ============================================================================

#[test]
fn plan_does_not_write_state() {
    let td = tempdir().expect("tempdir");
    create_single_crate_workspace(td.path());

    loopback_shipper_cmd()
        .arg("--manifest-path")
        .arg(td.path().join("Cargo.toml"))
        .arg("plan")
        .assert()
        .success();

    // plan should never create .shipper directory
    assert!(
        !td.path().join(".shipper").exists(),
        "plan should not create .shipper state directory"
    );
}

// ============================================================================
// Test 4: Publish with --package limits scope
// ============================================================================

#[test]
fn publish_with_package_flag_limits_scope() {
    let td = tempdir().expect("tempdir");
    create_workspace(td.path());
    let (new_path, real_cargo, fake_cargo) = setup_fake_cargo(td.path());

    // Only 1 crate: version-check 404 + readiness 200 = 2 requests
    let registry = spawn_registry(vec![404, 200], 2);

    let state_dir = td.path().join(".shipper");

    loopback_shipper_cmd()
        .arg("--manifest-path")
        .arg(td.path().join("Cargo.toml"))
        .arg("--api-base")
        .arg(&registry.base_url)
        .arg("--allow-dirty")
        .arg("--verify-timeout")
        .arg("0ms")
        .arg("--verify-poll")
        .arg("0ms")
        .arg("--max-attempts")
        .arg("1")
        .arg("--package")
        .arg("core")
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("publish")
        .env("PATH", &new_path)
        .env("REAL_CARGO", &real_cargo)
        .env("SHIPPER_CARGO_BIN", &fake_cargo)
        .env("SHIPPER_FAKE_PUBLISH_EXIT", "0")
        .assert()
        .success();

    // Receipt should only have core, not utils or app
    let receipt: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(state_dir.join("receipt.json")).expect("read receipt"),
    )
    .expect("parse");
    let packages = receipt["packages"].as_array().expect("packages");
    assert_eq!(packages.len(), 1, "only one package should be published");
    assert_eq!(packages[0]["name"].as_str(), Some("core"));
    assert_eq!(packages[0]["state"]["state"].as_str(), Some("published"));

    registry.join();
}

// ============================================================================
// Test 5: Publish creates events.jsonl with correct lifecycle events
// ============================================================================

#[test]
fn publish_creates_events_jsonl_with_lifecycle_events() {
    let td = tempdir().expect("tempdir");
    create_single_crate_workspace(td.path());
    let (new_path, real_cargo, fake_cargo) = setup_fake_cargo(td.path());

    let registry = spawn_registry(vec![404, 200], 2);
    let state_dir = td.path().join(".shipper");

    loopback_shipper_cmd()
        .arg("--manifest-path")
        .arg(td.path().join("Cargo.toml"))
        .arg("--api-base")
        .arg(&registry.base_url)
        .arg("--allow-dirty")
        .arg("--verify-timeout")
        .arg("0ms")
        .arg("--verify-poll")
        .arg("0ms")
        .arg("--max-attempts")
        .arg("1")
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("publish")
        .env("PATH", &new_path)
        .env("REAL_CARGO", &real_cargo)
        .env("SHIPPER_CARGO_BIN", &fake_cargo)
        .env("SHIPPER_FAKE_PUBLISH_EXIT", "0")
        .assert()
        .success();

    let events_path = state_dir.join("events.jsonl");
    assert!(events_path.exists(), "events.jsonl should be created");

    let events_content = fs::read_to_string(&events_path).expect("read events");
    assert!(
        !events_content.is_empty(),
        "events.jsonl should not be empty"
    );

    // Verify lifecycle events are present
    assert!(
        events_content.contains(r#""type":"plan_created"#),
        "should contain plan_created event"
    );
    assert!(
        events_content.contains(r#""type":"execution_started"#),
        "should contain execution_started event"
    );
    assert!(
        events_content.contains(r#""type":"package_started"#),
        "should contain package_started event"
    );
    assert!(
        events_content.contains(r#""type":"package_published"#),
        "should contain package_published event"
    );
    assert!(
        events_content.contains(r#""type":"execution_finished"#),
        "should contain execution_finished event"
    );

    // Each line should be valid JSON
    for line in events_content.lines() {
        let _: serde_json::Value =
            serde_json::from_str(line).expect("each events.jsonl line should be valid JSON");
    }

    registry.join();
}

// ============================================================================
// Test 6: Publish with custom --state-dir writes to correct location
// ============================================================================

#[test]
fn publish_with_custom_state_dir_writes_to_correct_location() {
    let td = tempdir().expect("tempdir");
    create_single_crate_workspace(td.path());
    let (new_path, real_cargo, fake_cargo) = setup_fake_cargo(td.path());

    let registry = spawn_registry(vec![404, 200], 2);
    let custom_dir = td.path().join("my-artifacts").join("nested");

    loopback_shipper_cmd()
        .arg("--manifest-path")
        .arg(td.path().join("Cargo.toml"))
        .arg("--api-base")
        .arg(&registry.base_url)
        .arg("--allow-dirty")
        .arg("--verify-timeout")
        .arg("0ms")
        .arg("--verify-poll")
        .arg("0ms")
        .arg("--max-attempts")
        .arg("1")
        .arg("--state-dir")
        .arg(&custom_dir)
        .arg("publish")
        .env("PATH", &new_path)
        .env("REAL_CARGO", &real_cargo)
        .env("SHIPPER_CARGO_BIN", &fake_cargo)
        .env("SHIPPER_FAKE_PUBLISH_EXIT", "0")
        .assert()
        .success();

    // All state files should be in the custom directory
    assert!(custom_dir.exists(), "custom state dir should be created");
    assert!(
        custom_dir.join("state.json").exists(),
        "state.json should be in custom dir"
    );
    assert!(
        custom_dir.join("receipt.json").exists(),
        "receipt.json should be in custom dir"
    );
    assert!(
        custom_dir.join("events.jsonl").exists(),
        "events.jsonl should be in custom dir"
    );

    // Default .shipper directory should NOT be created
    assert!(
        !td.path().join(".shipper").exists(),
        "default .shipper dir should not be created when custom --state-dir is used"
    );

    // Verify receipt content is correct
    let receipt: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(custom_dir.join("receipt.json")).expect("read receipt"),
    )
    .expect("parse receipt");
    assert!(receipt.get("plan_id").is_some());

    registry.join();
}

// ============================================================================
// Test 7: Failed publish creates appropriate state for resume
// ============================================================================

#[test]
fn failed_publish_creates_state_for_resume() {
    let td = tempdir().expect("tempdir");
    create_single_crate_workspace(td.path());
    let (new_path, real_cargo, fake_cargo) = setup_fake_cargo(td.path());

    // version-check 404 (not published), then registry confirms absence after
    // a permanent cargo failure.
    let registry = spawn_registry(vec![404, 404], 2);
    let state_dir = td.path().join(".shipper");

    loopback_shipper_cmd()
        .arg("--manifest-path")
        .arg(td.path().join("Cargo.toml"))
        .arg("--api-base")
        .arg(&registry.base_url)
        .arg("--allow-dirty")
        .arg("--verify-timeout")
        .arg("0ms")
        .arg("--verify-poll")
        .arg("0ms")
        .arg("--max-attempts")
        .arg("1")
        .arg("--base-delay")
        .arg("0ms")
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("publish")
        .env("PATH", &new_path)
        .env("REAL_CARGO", &real_cargo)
        .env("SHIPPER_CARGO_BIN", &fake_cargo)
        .env("SHIPPER_FAKE_PUBLISH_EXIT", "1")
        .env(
            "SHIPPER_FAKE_PUBLISH_STDERR",
            "error: not authorized to publish this crate",
        )
        .assert()
        .failure();

    // State file should still be created even after failure
    let state_path = state_dir.join("state.json");
    assert!(
        state_path.exists(),
        "state.json should exist after failed publish (for resume)"
    );

    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("read state"))
            .expect("parse state");
    assert!(state.get("plan_id").is_some(), "state should have plan_id");
    assert!(
        state.get("packages").is_some(),
        "state should have packages"
    );

    // The demo package should be in a non-published state (failed or pending)
    let packages = state["packages"].as_object().expect("packages map");
    let demo = packages.get("demo@0.1.0").expect("demo in packages");
    let demo_state = demo["state"]["state"].as_str().expect("state string");
    assert_ne!(
        demo_state, "published",
        "failed package should NOT be marked published"
    );

    registry.join();
}

// ============================================================================
// Test 8: Re-running publish when all crates already published skips everything
// ============================================================================

#[test]
fn publish_when_already_published_skips_all() {
    let td = tempdir().expect("tempdir");
    create_workspace(td.path());
    let (new_path, real_cargo, fake_cargo) = setup_fake_cargo(td.path());

    // Registry returns 200 for each version check (already published).
    let registry = spawn_registry(vec![200, 200, 200], 3);
    let state_dir = td.path().join(".shipper");
    let publish_log = td.path().join("publish.log");

    let output = loopback_shipper_cmd()
        .arg("--manifest-path")
        .arg(td.path().join("Cargo.toml"))
        .arg("--api-base")
        .arg(&registry.base_url)
        .arg("--allow-dirty")
        .arg("--verify-timeout")
        .arg("0ms")
        .arg("--verify-poll")
        .arg("0ms")
        .arg("--max-attempts")
        .arg("1")
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("--format")
        .arg("json")
        .arg("publish")
        .env("PATH", &new_path)
        .env("REAL_CARGO", &real_cargo)
        .env("SHIPPER_CARGO_BIN", &fake_cargo)
        .env("SHIPPER_FAKE_PUBLISH_EXIT", "0")
        .env("SHIPPER_FAKE_PUBLISH_LOG", &publish_log)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).expect("utf8");
    let report: serde_json::Value =
        serde_json::from_str(&stdout).expect("publish stdout should be command JSON");
    let report_packages = report["packages"].as_array().expect("report packages");

    assert_eq!(report_packages.len(), 3, "all packages should be reported");
    assert_eq!(report["published"].as_u64(), Some(0));
    assert_eq!(report["pending"].as_u64(), Some(0));
    assert_eq!(report["failed"].as_u64(), Some(0));
    assert_eq!(report["ambiguous"].as_u64(), Some(0));
    assert_eq!(report["uploaded"].as_u64(), Some(0));
    assert_eq!(report["skipped"].as_u64(), Some(3));
    for package in ["core", "utils", "app"] {
        assert_eq!(
            command_package_state(report_packages, package),
            "skipped",
            "{package} should be skipped in publish JSON"
        );
    }
    assert!(
        read_publish_log(&publish_log).is_empty(),
        "cargo publish must not run when every version already exists"
    );

    let receipt: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(state_dir.join("receipt.json")).expect("read receipt"),
    )
    .expect("parse");
    let packages = receipt["packages"].as_array().expect("packages");
    assert_eq!(packages.len(), 3, "all packages should be in receipt");
    for package in ["core", "utils", "app"] {
        assert_eq!(
            receipt_package_state(packages, package),
            "skipped",
            "{package} should be skipped in receipt"
        );
    }

    registry.join();
}

#[test]
fn publish_mixed_existing_and_missing_publishes_missing_only() {
    let td = tempdir().expect("tempdir");
    create_workspace(td.path());
    let (new_path, real_cargo, fake_cargo) = setup_fake_cargo(td.path());

    // core exists; utils and app are missing and become visible after publish.
    let registry = spawn_registry(vec![200, 404, 200, 404, 200], 5);
    let state_dir = td.path().join(".shipper");
    let publish_log = td.path().join("publish.log");

    let output = loopback_shipper_cmd()
        .arg("--manifest-path")
        .arg(td.path().join("Cargo.toml"))
        .arg("--api-base")
        .arg(&registry.base_url)
        .arg("--allow-dirty")
        .arg("--verify-timeout")
        .arg("0ms")
        .arg("--verify-poll")
        .arg("0ms")
        .arg("--max-attempts")
        .arg("1")
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("--format")
        .arg("json")
        .arg("publish")
        .env("PATH", &new_path)
        .env("REAL_CARGO", &real_cargo)
        .env("SHIPPER_CARGO_BIN", &fake_cargo)
        .env("SHIPPER_FAKE_PUBLISH_EXIT", "0")
        .env("SHIPPER_FAKE_PUBLISH_LOG", &publish_log)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).expect("utf8");
    let report: serde_json::Value =
        serde_json::from_str(&stdout).expect("publish stdout should be command JSON");
    let report_packages = report["packages"].as_array().expect("report packages");

    assert_eq!(command_package_state(report_packages, "core"), "skipped");
    assert_eq!(command_package_state(report_packages, "utils"), "published");
    assert_eq!(command_package_state(report_packages, "app"), "published");
    assert_eq!(report["published"].as_u64(), Some(2));
    assert_eq!(report["pending"].as_u64(), Some(0));
    assert_eq!(report["failed"].as_u64(), Some(0));
    assert_eq!(report["ambiguous"].as_u64(), Some(0));
    assert_eq!(report["uploaded"].as_u64(), Some(0));
    assert_eq!(report["skipped"].as_u64(), Some(1));

    let publish_log = read_publish_log(&publish_log);
    assert_eq!(
        publish_log.len(),
        2,
        "only missing package versions should invoke cargo publish"
    );
    assert!(
        publish_log[0].contains("-p utils"),
        "utils should publish after skipped core, log: {publish_log:?}"
    );
    assert!(
        publish_log[1].contains("-p app"),
        "app should publish after utils, log: {publish_log:?}"
    );
    assert!(
        publish_log.iter().all(|line| !line.contains("-p core")),
        "already-published core must not invoke cargo publish, log: {publish_log:?}"
    );

    let receipt: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(state_dir.join("receipt.json")).expect("read receipt"),
    )
    .expect("parse receipt");
    let packages = receipt["packages"].as_array().expect("receipt packages");
    assert_eq!(receipt_package_state(packages, "core"), "skipped");
    assert_eq!(receipt_package_state(packages, "utils"), "published");
    assert_eq!(receipt_package_state(packages, "app"), "published");

    registry.join();
}

#[test]
fn publish_mixed_existing_and_missing_failure_records_failed_package() {
    let td = tempdir().expect("tempdir");
    create_workspace(td.path());
    let (new_path, real_cargo, fake_cargo) = setup_fake_cargo(td.path());

    // core exists; utils is missing, cargo publish fails, registry confirms absent.
    let registry = spawn_registry(vec![200, 404, 404], 3);
    let state_dir = td.path().join(".shipper");
    let publish_log = td.path().join("publish.log");

    loopback_shipper_cmd()
        .timeout(Duration::from_secs(20))
        .arg("--manifest-path")
        .arg(td.path().join("Cargo.toml"))
        .arg("--api-base")
        .arg(&registry.base_url)
        .arg("--allow-dirty")
        .arg("--verify-timeout")
        .arg("0ms")
        .arg("--verify-poll")
        .arg("0ms")
        .arg("--max-attempts")
        .arg("1")
        .arg("--base-delay")
        .arg("0ms")
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("publish")
        .env("PATH", &new_path)
        .env("REAL_CARGO", &real_cargo)
        .env("SHIPPER_CARGO_BIN", &fake_cargo)
        .env("SHIPPER_FAKE_PUBLISH_EXIT", "1")
        .env(
            "SHIPPER_FAKE_PUBLISH_STDERR",
            "error: not authorized to publish this crate",
        )
        .env("SHIPPER_FAKE_PUBLISH_LOG", &publish_log)
        .assert()
        .failure();

    let publish_log = read_publish_log(&publish_log);
    assert_eq!(
        publish_log.len(),
        1,
        "failure should stop after first missing package publish attempt"
    );
    assert!(
        publish_log[0].contains("-p utils"),
        "utils should be the failed publish attempt, log: {publish_log:?}"
    );
    assert!(
        !publish_log[0].contains("-p core") && !publish_log[0].contains("-p app"),
        "already-published and downstream packages must not be published, log: {publish_log:?}"
    );

    let state: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(state_dir.join("state.json")).expect("read state"),
    )
    .expect("parse state");
    assert_eq!(
        state["packages"]["core@0.1.0"]["state"]["state"].as_str(),
        Some("skipped"),
        "already-published core should be recorded as skipped"
    );
    assert_eq!(
        state["packages"]["utils@0.1.0"]["state"]["state"].as_str(),
        Some("failed"),
        "failed missing package should be recorded as failed"
    );
    assert_eq!(
        state["packages"]["app@0.1.0"]["state"]["state"].as_str(),
        Some("pending"),
        "downstream package should remain pending after upstream failure"
    );

    registry.join();
}
