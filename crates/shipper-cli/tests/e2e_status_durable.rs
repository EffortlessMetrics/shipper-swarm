#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use assert_cmd::Command;
use chrono::Utc;
use shipper_core::lock::{LockFile, lock_path};
use shipper_core::plan::build_plan;
use shipper_core::state::events::{EventLog, events_path};
use shipper_core::state::execution_state::{
    RECEIPT_FILE, RECONCILIATION_FILE, save_state, save_state_encrypted, write_receipt_encrypted,
};
use shipper_core::state::rebuild::{StateRebuildOptions, rebuild_state_from_events};
use shipper_core::types::{
    EnvironmentFingerprint, ErrorClass, EventType, ExecutionResult, ExecutionState,
    PackageEvidence, PackageProgress, PackageReceipt, PackageState, PublishEvent, Receipt,
    ReconciliationOperatorAction, ReconciliationOutcome, ReconciliationRecord,
    ReconciliationReport, ReconciliationTrigger, Registry, ReleasePlan, ReleaseSpec,
};
use tempfile::tempdir;
use tiny_http::Server;

const SECRET: &str = "DURABLE_STATUS_MATRIX_SECRET_SENTINEL";

fn write(path: &Path, body: impl AsRef<[u8]>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, body).with_context(|| format!("write {}", path.display()))
}

fn create_workspace(root: &Path) -> Result<()> {
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"demo\"]\nresolver = \"2\"\n",
    )?;
    write(
        &root.join("demo/Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    write(&root.join("demo/src/lib.rs"), "pub fn demo() {}\n")
}

fn registry(base_url: &str) -> Registry {
    Registry {
        name: "crates-io".into(),
        api_base: base_url.into(),
        index_base: Some(base_url.into()),
    }
}

fn plan(root: &Path, base_url: &str) -> Result<ReleasePlan> {
    Ok(build_plan(&ReleaseSpec {
        manifest_path: root.join("Cargo.toml"),
        registry: registry(base_url),
        selected_packages: None,
    })?
    .plan)
}

fn event(event_type: EventType, package: &str) -> PublishEvent {
    PublishEvent {
        timestamp: Utc::now(),
        event_type,
        package: package.into(),
    }
}

fn write_events(
    state_dir: &Path,
    plan_id: &str,
    tail: impl IntoIterator<Item = PublishEvent>,
) -> Result<()> {
    let mut log = EventLog::new();
    log.record(event(EventType::ExecutionStarted, "workspace"));
    log.record(event(
        EventType::PlanCreated {
            plan_id: plan_id.into(),
            package_count: 1,
        },
        "workspace",
    ));
    for item in tail {
        log.record(item);
    }
    log.write_to_file(&events_path(state_dir))
}

fn pending_state(plan: &ReleasePlan, state: PackageState) -> ExecutionState {
    let now = Utc::now();
    ExecutionState {
        state_version: "shipper.state.v1".into(),
        plan_id: plan.plan_id.clone(),
        registry: plan.registry.clone(),
        created_at: now,
        updated_at: now,
        attempt_history: Vec::new(),
        packages: BTreeMap::from([(
            "demo@0.1.0".into(),
            PackageProgress {
                name: "demo".into(),
                version: "0.1.0".into(),
                attempts: 1,
                state,
                last_updated_at: now,
            },
        )]),
    }
}

fn terminal_evidence(state_dir: &Path, plan: &ReleasePlan) -> Result<(ExecutionState, Receipt)> {
    write_events(
        state_dir,
        &plan.plan_id,
        [
            event(EventType::PackagePublished { duration_ms: 1 }, "demo@0.1.0"),
            event(
                EventType::ExecutionFinished {
                    result: ExecutionResult::Success,
                },
                "workspace",
            ),
        ],
    )?;
    let state = rebuild_state_from_events(
        &events_path(state_dir),
        StateRebuildOptions::new(plan.registry.clone()).with_fallback_plan_id(&plan.plan_id),
    )?;
    let packages = state
        .packages
        .values()
        .map(|progress| PackageReceipt {
            name: progress.name.clone(),
            version: progress.version.clone(),
            attempts: progress.attempts,
            state: progress.state.clone(),
            started_at: state.created_at,
            finished_at: state.updated_at,
            duration_ms: 1,
            evidence: PackageEvidence {
                attempts: Vec::new(),
                readiness_checks: Vec::new(),
            },
            compromised_at: None,
            compromised_by: None,
            superseded_by: None,
        })
        .collect();
    let receipt = Receipt {
        receipt_version: "shipper.receipt.v2".into(),
        plan_id: plan.plan_id.clone(),
        registry: plan.registry.clone(),
        started_at: state.created_at,
        finished_at: state.updated_at,
        packages,
        event_log_path: PathBuf::from(".operator-state/events.jsonl"),
        git_context: None,
        environment: EnvironmentFingerprint {
            shipper_version: "test".into(),
            cargo_version: None,
            rust_version: None,
            os: "linux".into(),
            arch: std::env::consts::ARCH.into(),
        },
        auth_evidence: None,
        execution_result: ExecutionResult::Success,
    };
    Ok((state, receipt))
}

fn write_terminal(state_dir: &Path, plan: &ReleasePlan) -> Result<()> {
    let (state, receipt) = terminal_evidence(state_dir, plan)?;
    save_state(state_dir, &state)?;
    write(
        &state_dir.join(RECEIPT_FILE),
        serde_json::to_vec_pretty(&receipt)?,
    )
}

fn write_still_unknown(state_dir: &Path, plan: &ReleasePlan) -> Result<()> {
    let report = ReconciliationReport {
        schema_version: "shipper.reconciliation.v1".into(),
        plan_id: plan.plan_id.clone(),
        registry: plan.registry.clone(),
        generated_at: Utc::now(),
        evidence_sources: Vec::new(),
        records: vec![ReconciliationRecord {
            package: "demo@0.1.0".into(),
            name: "demo".into(),
            version: "0.1.0".into(),
            trigger: ReconciliationTrigger::ResumeAmbiguousState,
            method: None,
            cargo_exit_class: Some(ErrorClass::Ambiguous),
            outcome: still_unknown_outcome(),
            operator_action: ReconciliationOperatorAction::OperatorActionRequired,
        }],
    };
    write(
        &state_dir.join(RECONCILIATION_FILE),
        serde_json::to_vec_pretty(&report)?,
    )
}

fn still_unknown_outcome() -> ReconciliationOutcome {
    ReconciliationOutcome::StillUnknown {
        attempts: 1,
        elapsed_ms: 1,
        reason: "bounded fixture remains unknown".into(),
    }
}

fn write_not_live_lock(state_dir: &Path, root: &Path, plan_id: &str) -> Result<()> {
    let lock = LockFile::acquire(state_dir, Some(root))?;
    lock.set_plan_id(plan_id)?;
    let path = lock_path(state_dir, Some(root));
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    lock.release()?;
    value["pid"] = serde_json::json!(u32::MAX);
    write(&path, serde_json::to_vec_pretty(&value)?)
}

fn invoke(
    root: &Path,
    base_url: &str,
    state_name: &str,
    json: bool,
) -> Result<std::process::Output> {
    invoke_with_passphrase(root, base_url, state_name, json, None)
}

fn invoke_with_passphrase(
    root: &Path,
    base_url: &str,
    state_name: &str,
    json: bool,
    passphrase: Option<&str>,
) -> Result<std::process::Output> {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("shipper-cli"));
    command
        .timeout(Duration::from_secs(20))
        .current_dir(root)
        .env("CARGO_REGISTRY_TOKEN", SECRET)
        .arg("--allow-loopback")
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .arg("--api-base")
        .arg(base_url)
        .arg("--state-dir")
        .arg(state_name);
    if json {
        command.arg("--format").arg("json");
    }
    if let Some(passphrase) = passphrase {
        command
            .arg("--encrypt")
            .arg("--encrypt-passphrase")
            .arg(passphrase);
    }
    Ok(command.arg("status").arg("--durable").output()?)
}

fn assert_pair(
    root: &Path,
    base_url: &str,
    state_name: &str,
    status: &str,
    action: &str,
) -> Result<()> {
    let before = snapshot(root)?;
    let human = invoke(root, base_url, state_name, false)?;
    let json = invoke(root, base_url, state_name, true)?;
    ensure!(
        human.status.success(),
        "human stderr: {}",
        String::from_utf8_lossy(&human.stderr)
    );
    ensure!(
        json.status.success(),
        "json stderr: {}",
        String::from_utf8_lossy(&json.stderr)
    );
    let human_text = String::from_utf8(human.stdout)?;
    let value: serde_json::Value = serde_json::from_slice(&json.stdout)?;
    ensure!(value["schema_version"] == "shipper.status.durable.v1");
    ensure!(value["state_dir"] == state_name);
    ensure!(value["outcome"]["status"] == status);
    ensure!(value["outcome"]["next_action"]["kind"] == action);
    if action == "resume" {
        ensure!(value["outcome"]["next_action"].get("command").is_none());
        ensure!(value["outcome"]["safe_to_resume"]["value"] == true);
        ensure!(human_text.contains("Safe to resume: yes"));
    } else {
        ensure!(value["outcome"]["next_action"].get("command").is_none());
        ensure!(value["outcome"]["safe_to_resume"]["value"] == false);
        ensure!(human_text.contains("Safe to resume: no"));
    }
    ensure!(
        human_text
            .to_lowercase()
            .contains(&status.replace('_', " "))
    );
    for output in [
        &human_text,
        &String::from_utf8(human.stderr)?,
        &String::from_utf8(json.stderr)?,
        &value.to_string(),
    ] {
        ensure!(
            !output.contains(SECRET),
            "secret leaked from durable status"
        );
    }
    ensure!(
        snapshot(root)? == before,
        "durable status mutated fixture artifacts"
    );
    Ok(())
}

fn snapshot(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.insert(path.strip_prefix(root)?.to_path_buf(), fs::read(&path)?);
            }
        }
    }
    Ok(files)
}

#[test]
fn durable_status_process_matrix_is_fail_closed_and_side_effect_free() -> Result<()> {
    let td = tempdir()?;
    create_workspace(td.path())?;
    let server =
        Server::http("127.0.0.1:0").map_err(|_| anyhow::anyhow!("bind zero-request registry"))?;
    let base_url = format!("http://{}", server.server_addr());
    let plan = plan(td.path(), &base_url)?;

    let terminal = td.path().join("terminal-state");
    write_terminal(&terminal, &plan)?;
    let before = snapshot(td.path())?;
    assert_pair(
        td.path(),
        &base_url,
        "terminal-state",
        "terminal",
        "none_complete",
    )?;
    ensure!(
        snapshot(td.path())? == before,
        "terminal status mutated artifacts"
    );

    let encrypted = td.path().join("encrypted-state");
    let (encrypted_state, encrypted_receipt) = terminal_evidence(&encrypted, &plan)?;
    let encryption = shipper_core::encryption::EncryptionConfig::new("fixture-passphrase".into());
    save_state_encrypted(&encrypted, &encrypted_state, &encryption)?;
    write_receipt_encrypted(&encrypted, &encrypted_receipt, &encryption)?;
    let encrypted_before = snapshot(td.path())?;
    let encrypted_human = invoke_with_passphrase(
        td.path(),
        &base_url,
        "encrypted-state",
        false,
        Some("fixture-passphrase"),
    )?;
    let encrypted_json = invoke_with_passphrase(
        td.path(),
        &base_url,
        "encrypted-state",
        true,
        Some("fixture-passphrase"),
    )?;
    ensure!(encrypted_human.status.success());
    ensure!(encrypted_json.status.success());
    let encrypted_value: serde_json::Value = serde_json::from_slice(&encrypted_json.stdout)?;
    ensure!(encrypted_value["outcome"]["status"] == "terminal");
    ensure!(String::from_utf8(encrypted_human.stdout)?.contains("Durable result: terminal"));
    ensure!(
        snapshot(td.path())? == encrypted_before,
        "encrypted status mutated artifacts"
    );

    let resumable = td.path().join("resumable-state");
    write_events(&resumable, &plan.plan_id, [])?;
    save_state(&resumable, &pending_state(&plan, PackageState::Pending))?;
    write_not_live_lock(&resumable, td.path(), &plan.plan_id)?;
    assert_pair(
        td.path(),
        &base_url,
        "resumable-state",
        "interrupted",
        "resume",
    )?;

    let ambiguous = td.path().join("ambiguous-state");
    write_events(
        &ambiguous,
        &plan.plan_id,
        [
            event(EventType::PackageUploaded, "demo@0.1.0"),
            event(
                EventType::PublishReconciled {
                    outcome: still_unknown_outcome(),
                },
                "demo@0.1.0",
            ),
        ],
    )?;
    let ambiguous_state = rebuild_state_from_events(
        &events_path(&ambiguous),
        StateRebuildOptions::new(plan.registry.clone()).with_fallback_plan_id(&plan.plan_id),
    )?;
    save_state(&ambiguous, &ambiguous_state)?;
    write_not_live_lock(&ambiguous, td.path(), &plan.plan_id)?;
    write_still_unknown(&ambiguous, &plan)?;
    assert_pair(
        td.path(),
        &base_url,
        "ambiguous-state",
        "ambiguous",
        "reconcile",
    )?;

    let stale_ambiguous = td.path().join("stale-ambiguous-state");
    write_events(&stale_ambiguous, &plan.plan_id, [])?;
    save_state(
        &stale_ambiguous,
        &pending_state(&plan, PackageState::Uploaded),
    )?;
    write_not_live_lock(&stale_ambiguous, td.path(), &plan.plan_id)?;
    write_still_unknown(&stale_ambiguous, &plan)?;
    assert_pair(
        td.path(),
        &base_url,
        "stale-ambiguous-state",
        "evidence_disagreement",
        "stop_and_investigate",
    )?;

    let mismatch = td.path().join("mismatch-state");
    write_events(&mismatch, "other-plan", [])?;
    save_state(&mismatch, &pending_state(&plan, PackageState::Pending))?;
    assert_pair(
        td.path(),
        &base_url,
        "mismatch-state",
        "identity_mismatch",
        "stop_and_investigate",
    )?;

    let disagreement = td.path().join("disagreement-state");
    write_events(
        &disagreement,
        &plan.plan_id,
        [event(
            EventType::ExecutionFinished {
                result: ExecutionResult::PartialFailure,
            },
            "workspace",
        )],
    )?;
    save_state(&disagreement, &pending_state(&plan, PackageState::Uploaded))?;
    assert_pair(
        td.path(),
        &base_url,
        "disagreement-state",
        "evidence_disagreement",
        "stop_and_investigate",
    )?;

    let live = td.path().join("live-state");
    write_events(&live, &plan.plan_id, [])?;
    save_state(&live, &pending_state(&plan, PackageState::Pending))?;
    let live_lock = LockFile::acquire(&live, Some(td.path()))?;
    live_lock.set_plan_id(&plan.plan_id)?;
    assert_pair(td.path(), &base_url, "live-state", "live", "status")?;
    live_lock.release()?;

    let corrupt = td.path().join("corrupt-state");
    write(&events_path(&corrupt), b"not-json\n")?;
    for json in [false, true] {
        let output = invoke(td.path(), &base_url, "corrupt-state", json)?;
        ensure!(output.status.code() == Some(1));
        ensure!(output.stdout.is_empty());
        ensure!(String::from_utf8(output.stderr)?.contains("failed to observe durable run"));
    }

    ensure!(
        server.recv_timeout(Duration::from_millis(250))?.is_none(),
        "durable status queried registry"
    );
    ensure!(
        !snapshot(td.path())?
            .values()
            .any(|body| String::from_utf8_lossy(body).contains(SECRET)),
        "secret leaked to artifacts"
    );
    Ok(())
}
