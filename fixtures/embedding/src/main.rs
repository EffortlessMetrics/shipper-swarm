#![forbid(unsafe_code)]

use std::{collections::BTreeSet, fs, path::Path, thread, time::Duration};

use anyhow::{Context, Result, anyhow, bail, ensure};
use shipper::{
    config::{CliOverrides, ShipperConfig, into_runtime_options},
    engine::{PreflightRunOptions, Reporter, run_preflight_with_options},
    plan::build_plan,
    state::events::{EventLog, events_path, preflight_only_events_paths},
    types::{EventType, Finishability, PublishEvent, Registry, ReleaseSpec, VerifyMode},
};
use tiny_http::{Method, Response, Server};

const BASE: &str = "z-embedding-base";
const APP: &str = "a-embedding-app";

struct ConsoleReporter;

impl Reporter for ConsoleReporter {
    fn info(&mut self, message: &str) {
        println!("{message}");
    }

    fn warn(&mut self, message: &str) {
        eprintln!("warning: {message}");
    }

    fn error(&mut self, message: &str) {
        eprintln!("error: {message}");
    }
}

fn main() -> Result<()> {
    let temp = tempfile::tempdir().context("create scratch workspace")?;
    let manifest_path = temp.path().join("Cargo.toml");
    write_workspace(temp.path()).context("write scratch workspace")?;

    let server = Server::http("127.0.0.1:0").map_err(|error| anyhow!(error))?;
    let endpoint = format!("http://{}", server.server_addr());
    let spec = ReleaseSpec {
        manifest_path,
        registry: Registry {
            name: "shipper-embedding-fixture".into(),
            api_base: endpoint.clone(),
            index_base: Some(endpoint),
        },
        selected_packages: None,
    };
    let workspace = build_plan(&spec).context("build release plan")?;
    let order: Vec<_> = workspace
        .plan
        .packages
        .iter()
        .map(|package| package.name.as_str())
        .collect();
    ensure!(order == [BASE, APP], "unexpected plan order: {order:?}");
    ensure!(
        build_plan(&spec)?.plan.plan_id == workspace.plan.plan_id,
        "identical input produced a different plan ID"
    );

    let missing = "missing-package";
    let invalid_spec = ReleaseSpec {
        selected_packages: Some(vec![missing.into()]),
        ..spec.clone()
    };
    match build_plan(&invalid_spec) {
        Err(error) if error.to_string().contains(missing) => {
            println!("Select a publishable workspace member and rebuild the plan ({error}).");
        }
        Err(error) => return Err(error).context("expected a missing-package error"),
        Ok(_) => bail!("planning accepted an unknown package"),
    }

    let state_dir = temp.path().join("evidence");
    let options = into_runtime_options(ShipperConfig::default().build_runtime_options(
        CliOverrides {
            allow_dirty: true,
            allow_loopback: true,
            registry_name: Some(spec.registry.name.clone()),
            skip_ownership_check: true,
            verify_mode: Some(VerifyMode::None),
            state_dir: Some(state_dir.clone()),
            ..CliOverrides::default()
        },
    ));

    let mock_registry = thread::spawn(move || serve_registry(server));
    let report = run_preflight_with_options(
        &workspace,
        &options,
        &mut ConsoleReporter,
        PreflightRunOptions { fresh_audit: true },
    )
    .context("run local preflight");
    let served = mock_registry
        .join()
        .map_err(|_| anyhow!("mock registry thread panicked"))?;
    let report = report?;
    served?;
    ensure!(
        report.plan_id == workspace.plan.plan_id,
        "report belongs to another plan"
    );
    ensure!(
        report.packages.len() == order.len()
            && order.iter().all(|name| {
                report.packages.iter().any(|package| {
                    package.name == *name && package.is_new_crate && !package.already_published
                })
            }),
        "preflight did not observe both versions absent from the mock registry"
    );

    let audits = preflight_only_events_paths(&state_dir)?;
    let [audit] = audits.as_slice() else {
        bail!(
            "expected one session-scoped event log, found {}",
            audits.len()
        );
    };
    ensure!(
        !events_path(&state_dir).exists() && !state_dir.join("state.json").exists(),
        "fresh audit wrote execution evidence"
    );
    let events = EventLog::read_from_file(audit)?;
    let completion = preflight_completion(events.all_events(), &report.finishability)?;
    let json = serde_json::to_string(completion)?;
    let decoded: PublishEvent = serde_json::from_str(&json)?;
    ensure!(
        decoded.timestamp == completion.timestamp && decoded.package == completion.package,
        "event round trip changed its timestamp or package"
    );
    preflight_completion(&[decoded], &report.finishability)?;
    println!("Preflight did not establish finishability; ownership was not checked. Event: {json}");
    Ok(())
}

fn preflight_completion<'a>(
    events: &'a [PublishEvent],
    reported: &Finishability,
) -> Result<&'a PublishEvent> {
    match events.last() {
        Some(event)
            if event.package == "all"
                && matches!(
                    &event.event_type,
                    EventType::PreflightComplete { finishability }
                        if *finishability == Finishability::NotProven && finishability == reported
                ) =>
        {
            Ok(event)
        }
        _ => bail!("expected terminal preflight evidence agreeing with the NotProven report"),
    }
}

fn write_workspace(root: &Path) -> Result<()> {
    fs::write(
        root.join("Cargo.toml"),
        format!("[workspace]\nresolver = \"3\"\nmembers = [\"{APP}\", \"{BASE}\"]\n"),
    )?;
    for (name, dependencies) in [
        (BASE, String::new()),
        (
            APP,
            format!("[dependencies]\n{BASE} = {{ path = \"../{BASE}\", version = \"0.1.0\" }}\n"),
        ),
    ] {
        let package = root.join(name);
        fs::create_dir_all(package.join("src"))?;
        fs::write(
            package.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n{dependencies}"
            ),
        )?;
        fs::write(package.join("src/lib.rs"), "")?;
    }
    Ok(())
}

fn serve_registry(server: Server) -> Result<()> {
    let mut expected: BTreeSet<_> = [BASE, APP]
        .into_iter()
        .flat_map(|package| {
            [
                format!("/api/v1/crates/{package}"),
                format!("/api/v1/crates/{package}/0.1.0"),
            ]
        })
        .collect();
    while !expected.is_empty() {
        let request = server
            .recv_timeout(Duration::from_secs(5))?
            .with_context(|| format!("timed out waiting for registry reads: {expected:?}"))?;
        ensure!(
            request.method() == &Method::Get && expected.remove(request.url()),
            "unexpected registry request: {} {}",
            request.method(),
            request.url()
        );
        request.respond(Response::empty(404))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_or_disagreeing_completion() -> Result<()> {
        let event: PublishEvent = serde_json::from_str(
            r#"{"timestamp":"2026-01-01T00:00:00Z","event_type":{"type":"preflight_complete","finishability":"not_proven"},"package":"all"}"#,
        )?;
        ensure!(preflight_completion(&[], &Finishability::NotProven).is_err());
        ensure!(
            preflight_completion(std::slice::from_ref(&event), &Finishability::Proven).is_err()
        );
        preflight_completion(&[event], &Finishability::NotProven)?;
        Ok(())
    }
}
