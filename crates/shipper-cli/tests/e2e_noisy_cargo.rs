//! Prove that both schedulers use the concurrently drained Cargo process path.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tiny_http::{Response, Server, StatusCode};

const STDOUT_SECRET: &str = "noisy-stdout-secret-sentinel";
const STDERR_SECRET: &str = "noisy-stderr-secret-sentinel";

#[test]
#[ignore = "fake Cargo subprocess invoked by the noisy-output CLI tests"]
fn noisy_cargo_child() {
    // The old runner can block this helper behind a full pipe. Its own watchdog
    // bounds a failing control even on platforms where a command script leaves
    // this process running after the script itself is killed.
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(15));
        std::process::exit(99);
    });
    let root = PathBuf::from(std::env::var_os("SHIPPER_NOISY_FIXTURE_ROOT").expect("fixture root"));
    let mut calls = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("publish-calls"))
        .expect("publish calls");
    writeln!(calls, "publish").expect("record publish");
    let mut stdout = std::io::stdout().lock();
    let mut stderr = std::io::stderr().lock();
    let out = format!("{}\n", "O".repeat(4095));
    let err = format!("{}\n", "E".repeat(4095));
    for _ in 0..256 {
        stdout.write_all(out.as_bytes()).unwrap();
        stderr.write_all(err.as_bytes()).unwrap();
    }
    writeln!(stdout, "Authorization: Bearer {STDOUT_SECRET}").unwrap();
    writeln!(stdout, "stdout complete").unwrap();
    writeln!(stderr, "Token={STDERR_SECRET}").unwrap();
    writeln!(stderr, "stderr complete").unwrap();
    stdout.flush().unwrap();
    stderr.flush().unwrap();
    fs::write(root.join("published"), "complete").expect("publish marker");
    std::process::exit(0);
}

struct Registry {
    base_url: String,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<(usize, usize)>>,
}

impl Registry {
    fn start(root: &Path) -> Self {
        let server = Server::http("127.0.0.1:0").expect("loopback registry");
        let base_url = format!("http://{}", server.server_addr());
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let published = root.join("published");
        let handle = thread::spawn(move || {
            let (mut missing, mut present) = (0, 0);
            while !worker_stop.load(Ordering::Acquire) {
                let Some(request) = server
                    .recv_timeout(Duration::from_millis(50))
                    .expect("registry request")
                else {
                    continue;
                };
                assert_eq!(request.url(), "/api/v1/crates/demo/0.1.0");
                let status = if published.is_file() {
                    present += 1;
                    200
                } else {
                    missing += 1;
                    404
                };
                request
                    .respond(Response::from_string("{}").with_status_code(StatusCode(status)))
                    .expect("registry response");
            }
            (missing, present)
        });
        Self {
            base_url,
            stop,
            handle: Some(handle),
        }
    }

    fn finish(mut self) -> (usize, usize) {
        self.stop.store(true, Ordering::Release);
        self.handle.take().unwrap().join().expect("registry worker")
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn create_fixture(root: &Path) -> PathBuf {
    fs::create_dir_all(root.join("demo/src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"demo\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    fs::write(
        root.join("demo/Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(root.join("demo/src/lib.rs"), "pub fn demo() {}\n").unwrap();

    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;

        let proxy = root.join("fake-cargo");
        fs::write(
            &proxy,
            "#!/bin/sh\nif [ \"$1\" = publish ]; then\n  exec \"$SHIPPER_NOISY_FIXTURE_EXE\" --exact noisy_cargo_child --ignored --nocapture\nfi\nexec \"$REAL_CARGO\" \"$@\"\n",
        )
        .unwrap();
        fs::set_permissions(&proxy, fs::Permissions::from_mode(0o755)).unwrap();
        proxy
    }
    #[cfg(windows)]
    {
        let proxy = root.join("fake-cargo.cmd");
        fs::write(
            &proxy,
            "@echo off\r\nif \"%~1\"==\"publish\" (\r\n  \"%SHIPPER_NOISY_FIXTURE_EXE%\" --exact noisy_cargo_child --ignored --nocapture\r\n  exit /b\r\n)\r\n\"%REAL_CARGO%\" %*\r\nexit /b\r\n",
        )
        .unwrap();
        proxy
    }
}

fn publish_with_noisy_cargo(parallel: bool) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let proxy = create_fixture(root);
    let registry = Registry::start(root);
    let state_dir = root.join("state");
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("shipper-cli"));
    command
        .timeout(Duration::from_secs(20))
        .current_dir(root)
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("--api-base")
        .arg(&registry.base_url)
        .args([
            "--allow-loopback",
            "--allow-dirty",
            "--skip-ownership-check",
            "--no-verify",
            "--max-attempts",
            "1",
            "--output-lines",
            "2",
            "--per-package-timeout",
            "5s",
            "--readiness-method",
            "api",
            "--readiness-poll",
            "0ms",
            "--readiness-timeout",
            "1s",
        ])
        .env("SHIPPER_CARGO_BIN", proxy)
        .env(
            "REAL_CARGO",
            std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()),
        )
        .env(
            "SHIPPER_NOISY_FIXTURE_EXE",
            std::env::current_exe().unwrap(),
        )
        .env("SHIPPER_NOISY_FIXTURE_ROOT", root);
    if parallel {
        command.args(["--parallel", "--max-concurrent", "2"]);
    }
    let output = command.arg("publish").output();
    let requests = registry.finish();
    let output = output.unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        requests.0 > 0 && requests.1 > 0,
        "registry phases: {requests:?}"
    );
    assert_eq!(
        fs::read_to_string(root.join("publish-calls")).unwrap(),
        "publish\n"
    );

    let receipt_text = fs::read_to_string(state_dir.join("receipt.json")).unwrap();
    let receipt: Value = serde_json::from_str(&receipt_text).unwrap();
    assert_eq!(receipt["execution_result"], "success");
    let package = &receipt["packages"][0];
    assert_eq!(package["state"]["state"], "published");
    let attempts = package["evidence"]["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 1);
    let attempt = &attempts[0];
    assert_eq!(attempt["exit_code"], 0);
    assert_eq!(
        attempt["stdout_tail"],
        "Authorization: [REDACTED]\nstdout complete"
    );
    assert_eq!(attempt["stderr_tail"], "Token=[REDACTED]\nstderr complete");

    let events_text = fs::read_to_string(state_dir.join("events.jsonl")).unwrap();
    let events: Vec<Value> = events_text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let outputs: Vec<_> = events
        .iter()
        .filter(|event| event["event_type"]["type"] == "package_output")
        .collect();
    assert_eq!(outputs.len(), 1);
    assert_eq!(
        outputs[0]["event_type"]["stdout_tail"],
        attempt["stdout_tail"]
    );
    assert_eq!(
        outputs[0]["event_type"]["stderr_tail"],
        attempt["stderr_tail"]
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event_type"]["type"] == "package_attempted")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event["event_type"]["type"] == "package_published")
            .count(),
        1
    );

    let state_text = fs::read_to_string(state_dir.join("state.json")).unwrap();
    let state: Value = serde_json::from_str(&state_text).unwrap();
    assert_eq!(state["packages"]["demo@0.1.0"]["attempts"], 1);
    assert_eq!(
        state["packages"]["demo@0.1.0"]["state"]["state"],
        "published"
    );
    for text in [
        receipt_text,
        events_text,
        state_text,
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ] {
        assert!(!text.contains(STDOUT_SECRET) && !text.contains(STDERR_SECRET));
        assert!(!text.contains("timed out"));
    }
}

#[test]
#[serial_test::serial]
fn sequential_publish_captures_noisy_cargo_without_false_timeout() {
    publish_with_noisy_cargo(false);
}

#[test]
#[serial_test::serial]
fn parallel_publish_captures_noisy_cargo_without_false_timeout() {
    publish_with_noisy_cargo(true);
}
