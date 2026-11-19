//! Encrypted execution admission must preserve evidence and never launch a
//! worker. Cargo is instrumented on Unix to distinguish discovery from work.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;

const SECRET: &str = "synthetic-encryption-test-only";

fn invoke(root: &Path) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("shipper-cli"));
    cmd.current_dir(root)
        .env("CARGO", root.join("log-cargo"))
        .env("SHIPPER_SYNTHETIC_REAL_CARGO", env!("CARGO"))
        .env("CARGO_HOME", root.join("cargo-home"))
        .env("CARGO_NET_OFFLINE", "true")
        .env_remove("CARGO_REGISTRY_TOKEN")
        .args(["--allow-loopback", "--api-base", "http://127.0.0.1:1"])
        .args([
            "--manifest-path",
            "member/Cargo.toml",
            "--state-dir",
            "evidence",
        ])
        .timeout(Duration::from_secs(10));
    cmd
}

fn fixture(root: &Path) {
    fs::create_dir_all(root.join("member/src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = ['member']\nresolver = '3'\n",
    )
    .unwrap();
    fs::write(
        root.join("member/Cargo.toml"),
        "[package]\nname = 'encryption-fixture'\nversion = '0.0.0'\nedition = '2024'\n[dependencies]\nnot-a-real-fixture-dependency = '*'\n",
    )
    .unwrap();
    fs::write(root.join("member/src/lib.rs"), "").unwrap();
    fs::write(
        root.join("log-cargo"),
        r#"#!/bin/sh
printf '%s\n' "$1" >> cargo-calls
if [ "$1" = locate-project ]; then
    exec "$SHIPPER_SYNTHETIC_REAL_CARGO" "$@"
fi
exit 91
"#,
    )
    .unwrap();
    fs::set_permissions(root.join("log-cargo"), fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
#[serial_test::serial]
fn encryption_refuses_all_evidence_writers_before_cargo_for_explicit_flag() {
    let td = tempfile::tempdir().unwrap();
    fixture(td.path());
    for args in [
        &["publish"][..],
        &["resume"],
        &["preflight", "--preflight-only"],
        &["rehearse"],
        &[
            "yank",
            "--crate",
            "encryption-fixture",
            "--version",
            "0.0.0",
        ],
        &[
            "remediate",
            "--dry-run",
            "--crate",
            "encryption-fixture",
            "--target-version",
            "0.0.0",
            "--reason",
            "fixture only",
        ],
    ] {
        let output = invoke(td.path())
            .args(["--encrypt", "--encrypt-passphrase", SECRET])
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.contains("encrypted execution evidence is not supported"),
            "{args:?}: {stderr}"
        );
        assert!(!stderr.contains(SECRET));
        assert!(output.stdout.is_empty());
    }
    assert!(!td.path().join("cargo-calls").exists());
    assert!(!td.path().join("evidence").exists());
}

#[test]
#[serial_test::serial]
fn explicit_encryption_config_refuses_publish_with_existing_json_error_shape() {
    let td = tempfile::tempdir().unwrap();
    fixture(td.path());
    fs::write(
        td.path().join("custom.toml"),
        format!("[encryption]\nenabled = true\npassphrase = '{SECRET}'\n"),
    )
    .unwrap();
    let output = invoke(td.path())
        .args(["--config", "custom.toml", "--format", "json", "publish"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.contains(SECRET));
    let report: Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(report["schema_version"], "shipper.publish.error.v1");
    assert_eq!(report["category"], "unsupported_state_encryption");
    assert_eq!(report["next_action"]["kind"], "resolve_blockers");
    assert_eq!(report["evidence"], serde_json::json!([]));
    assert!(!td.path().join("cargo-calls").exists());
    assert!(!td.path().join("evidence").exists());
}

#[test]
#[serial_test::serial]
fn implicit_workspace_policy_permits_only_discovery_and_preserves_resume_bytes() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path();
    fixture(root);
    fs::write(root.join(".shipper.toml"), "[encryption]\nenabled = true\n").unwrap();
    fs::write(
        root.join("member/.shipper.toml"),
        "[encryption]\nenabled = false\n",
    )
    .unwrap();
    let before = [
        ("state.json", "retained state bytes"),
        ("receipt.json", "retained receipt bytes"),
        ("events.jsonl", "retained event bytes"),
        ("lock.guard", "retained guard bytes"),
    ];
    fs::create_dir(root.join("evidence")).unwrap();
    for (file, bytes) in before {
        fs::write(root.join("evidence").join(file), bytes).unwrap();
    }
    let output = invoke(root).arg("resume").output().unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("encrypted execution evidence is not supported")
    );
    assert_eq!(
        fs::read_to_string(root.join("cargo-calls")).unwrap(),
        "locate-project\n"
    );
    assert!(!root.join("Cargo.lock").exists());
    assert_eq!(
        fs::read_dir(root.join("evidence")).unwrap().count(),
        before.len()
    );
    for (file, bytes) in before {
        assert_eq!(
            fs::read_to_string(root.join("evidence").join(file)).unwrap(),
            bytes
        );
    }

    // Explicit plaintext config still wins over the implicit workspace config.
    // The fake Cargo stops at the full planning step, before any publish work.
    let control = invoke(root)
        .args(["--config", "member/.shipper.toml", "publish"])
        .output()
        .unwrap();
    assert!(!control.status.success());
    assert_eq!(
        fs::read_to_string(root.join("cargo-calls")).unwrap(),
        "locate-project\nmetadata\n"
    );
}
