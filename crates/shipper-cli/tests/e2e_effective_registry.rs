//! Effective registry planning must agree across inspection and execution.
//! All registry traffic stays on loopback. Fake Cargo creates visibility
//! markers only after a successful simulated upload, never from a probe count.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::{TempDir, tempdir};
use tiny_http::{Response, Server, StatusCode};

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    fs::write(path, text).expect("write fixture");
}

struct Fixture {
    root: TempDir,
    fake_cargo: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempdir().expect("workspace");
        write(
            &root.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"common\", \"private-only\"]\nresolver = \"2\"\n",
        );
        for (name, extra) in [
            ("common", ""),
            (
                "private-only",
                "publish = [\"private-reg\"]\n\n[dependencies]\ncommon = { path = \"../common\", version = \"0.1.0\" }\n",
            ),
        ] {
            write(
                &root.path().join(name).join("Cargo.toml"),
                &format!(
                    "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{extra}"
                ),
            );
            write(
                &root.path().join(name).join("src/lib.rs"),
                "pub fn demo() {}\n",
            );
        }
        write(
            &root.path().join("empty.toml"),
            "schema_version = \"shipper.config.v1\"\n",
        );
        fs::create_dir(root.path().join("cargo-home")).expect("isolated Cargo home");
        let fake_cargo = write_fake_cargo(root.path());
        Self { root, fake_cargo }
    }

    fn root(&self) -> &Path {
        self.root.path()
    }

    fn command(&self) -> Command {
        self.command_with_format("json")
    }

    fn command_with_format(&self, format: &str) -> Command {
        let mut command = Command::new(assert_cmd::cargo::cargo_bin!("shipper-cli"));
        command
            .current_dir(self.root())
            .timeout(Duration::from_secs(20))
            .args([
                "--format",
                format,
                "--allow-dirty",
                "--skip-ownership-check",
            ])
            .env("CARGO_HOME", self.root().join("cargo-home"))
            .env("CARGO_NET_OFFLINE", "true")
            .env("SHIPPER_CARGO_BIN", &self.fake_cargo)
            .env("SHIPPER_TEST_CARGO_LOG", self.root().join("cargo.log"))
            .env("SHIPPER_TEST_MARKERS", self.root().join("markers"))
            .env_remove("SHIPPER_ENCRYPT_KEY")
            .env_remove("ACTIONS_ID_TOKEN_REQUEST_URL")
            .env_remove("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
            .env_remove("CARGO_REGISTRY_TOKEN")
            .env_remove("CARGO_REGISTRIES_CRATES_IO_TOKEN")
            .env_remove("CARGO_REGISTRIES_PRIVATE_REG_TOKEN");
        command
    }

    fn config(&self, api: &str) {
        write(
            &self.root().join(".shipper.toml"),
            &format!(
                "schema_version = \"shipper.config.v1\"\n[registry]\nname = \"private-reg\"\napi_base = \"{api}\"\nindex_base = \"{api}\"\n"
            ),
        );
    }

    fn catalog(&self, public_api: &str, private_api: &str) {
        write(
            &self.root().join(".shipper.toml"),
            &format!(
                "schema_version = \"shipper.config.v1\"\n\
                 [[registries.registries]]\nname = \"crates-io\"\napi_base = \"{public_api}\"\nindex_base = \"{public_api}\"\n\
                 [[registries.registries]]\nname = \"private-reg\"\napi_base = \"{private_api}\"\nindex_base = \"{private_api}\"\n"
            ),
        );
    }

    fn assert_no_execution(&self) {
        assert!(
            !self.root().join("cargo.log").exists(),
            "no Cargo publish or dry-run"
        );
        assert!(
            !self.root().join(".shipper").exists(),
            "no command state or events"
        );
        assert!(
            !self.root().join("markers").exists(),
            "no simulated uploads"
        );
    }
}

fn json(command: &mut Command) -> Value {
    let output = command.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&output).expect("one JSON document")
}

fn package_names(report: &Value) -> Vec<&str> {
    report["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .map(|package| package["name"].as_str().expect("package name"))
        .collect()
}

#[test]
fn config_and_flags_plan_the_same_private_selection_and_existing_id_contract() {
    let fixture = Fixture::new();
    fixture.config("https://private.example/api");
    let configured = json(
        fixture
            .command()
            .args(["plan", "--package", "private-only"]),
    );
    assert_eq!(package_names(&configured), ["common", "private-only"]);
    assert_eq!(
        configured["packages"][1]["dependencies"],
        serde_json::json!(["common@0.1.0"])
    );
    // SHA256(API + '\n' + ordered name@version lines), the existing contract.
    assert_eq!(
        configured["plan_id"],
        "57b43f48642db4a8b672ff5971b1be891c5a4de7fdc29db63a135ca39e301ba9"
    );

    let explicit = json(fixture.command().args([
        "--config",
        "empty.toml",
        "--registry",
        "private-reg",
        "--api-base",
        "https://private.example/api",
        "plan",
        "--package",
        "private-only",
    ]));
    assert_eq!(
        configured, explicit,
        "config and flags must produce identical reports"
    );

    let member = json(
        fixture
            .command()
            .args(["--manifest-path"])
            .arg(fixture.root().join("private-only/Cargo.toml"))
            .current_dir(fixture.root().join("private-only"))
            .args(["plan", "--package", "private-only"]),
    );
    assert_eq!(
        configured, member,
        "discover config at the actual workspace root"
    );

    let config_path = fixture.root().join("explicit.toml");
    fs::rename(fixture.root().join(".shipper.toml"), &config_path).expect("move config");
    let explicit_config = json(fixture.command().arg("--config").arg(&config_path).args([
        "plan",
        "--package",
        "private-only",
    ]));
    assert_eq!(configured, explicit_config);

    let changed = json(fixture.command().arg("--config").arg(&config_path).args([
        "--api-base",
        "https://other.example/api",
        "plan",
        "--package",
        "private-only",
    ]));
    assert_eq!(package_names(&changed), package_names(&configured));
    assert_eq!(
        changed["plan_id"],
        "440e9c2bc21a4614545ba65c7a74a41dc563ea81a166295c0c7df0357c8b7508"
    );
    fixture.assert_no_execution();
}

struct LocalRegistry {
    api: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl LocalRegistry {
    fn new(fixture: &Fixture, name: &str) -> Self {
        let server = Server::http("127.0.0.1:0").expect("loopback registry");
        let api = format!("http://{}", server.server_addr());
        let markers = fixture.root().join("markers").join(name);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let requests_in_thread = Arc::clone(&requests);
        let stop_in_thread = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(60);
            while !stop_in_thread.load(Ordering::Relaxed) && Instant::now() < deadline {
                let Some(request) = server
                    .recv_timeout(Duration::from_millis(50))
                    .expect("receive")
                else {
                    continue;
                };
                let path = request.url().to_string();
                assert_eq!(request.method().as_str(), "GET", "no registry mutations");
                requests_in_thread
                    .lock()
                    .expect("requests")
                    .push(path.clone());
                let package = path
                    .strip_prefix("/api/v1/crates/")
                    .and_then(|tail| tail.split('/').next());
                let visible = package.is_some_and(|package| markers.join(package).exists());
                request
                    .respond(
                        Response::from_string("{}").with_status_code(StatusCode(if visible {
                            200
                        } else {
                            404
                        })),
                    )
                    .expect("respond");
            }
        });
        Self {
            api,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn assert_no_requests(&self) {
        assert!(
            self.requests.lock().expect("requests").is_empty(),
            "planning/refusal must not contact the registry"
        );
    }
}

impl Drop for LocalRegistry {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread
            .take()
            .expect("registry thread")
            .join()
            .expect("join registry");
    }
}

fn write_fake_cargo(root: &Path) -> PathBuf {
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = root.join("fake-cargo");
        write(
            &path,
            r#"#!/usr/bin/env sh
printf '%s\n' "$*" >> "$SHIPPER_TEST_CARGO_LOG"
[ "$1" = publish ] || exit 0
registry=crates-io
package=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --dry-run) exit 0 ;;
    --registry) shift; registry="$1" ;;
    -p|--package) shift; package="$1" ;;
  esac
  shift
done
if [ "$package" = private-only ] && [ "${SHIPPER_TEST_FAIL_PRIVATE:-0}" = 1 ]; then
  echo 'network timeout while uploading' >&2
  exit 1
fi
[ -n "$package" ] || exit 1
mkdir -p "$SHIPPER_TEST_MARKERS/$registry"
: > "$SHIPPER_TEST_MARKERS/$registry/$package"
"#,
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("executable");
        path
    }
    #[cfg(windows)]
    {
        let path = root.join("fake-cargo.cmd");
        write(
            &path,
            r#"@echo off
setlocal EnableDelayedExpansion
echo %*>>"%SHIPPER_TEST_CARGO_LOG%"
if not "%1"=="publish" exit /b 0
set registry=crates-io
set package=
:args
if "%1"=="" goto publish
if "%1"=="--dry-run" exit /b 0
if "%1"=="--registry" (set registry=%2& shift)
if "%1"=="-p" (set package=%2& shift)
if "%1"=="--package" (set package=%2& shift)
shift
goto args
:publish
if "!package!"=="private-only" if "%SHIPPER_TEST_FAIL_PRIVATE%"=="1" (
  echo network timeout while uploading 1>&2
  exit /b 1
)
if "!package!"=="" exit /b 1
if not exist "%SHIPPER_TEST_MARKERS%\!registry!" mkdir "%SHIPPER_TEST_MARKERS%\!registry!"
type nul > "%SHIPPER_TEST_MARKERS%\!registry!\!package!"
exit /b 0
"#,
        );
        path
    }
}

#[test]
fn one_catalog_target_agrees_with_flags_status_and_preflight() {
    let fixture = Fixture::new();
    let registry = LocalRegistry::new(&fixture, "private-reg");
    fixture.catalog("https://crates.io", &registry.api);
    let planned =
        json(
            fixture
                .command()
                .args(["--allow-loopback", "--registries", "private-reg", "plan"]),
        );
    let explicit = json(fixture.command().args([
        "--allow-loopback",
        "--registry",
        "private-reg",
        "--api-base",
        &registry.api,
        "plan",
    ]));
    assert_eq!(planned, explicit);
    assert_eq!(package_names(&planned), ["common", "private-only"]);
    registry.assert_no_requests();
    fixture.assert_no_execution();

    let status =
        json(
            fixture
                .command()
                .args(["--allow-loopback", "--registries", "private-reg", "status"]),
        );
    assert_eq!(status["plan_id"], planned["plan_id"]);
    assert_eq!(
        package_names(&status["registries"][0]),
        package_names(&planned)
    );

    let preflight = json(fixture.command().args([
        "--allow-loopback",
        "--registries",
        "private-reg",
        "--verify-mode",
        "package",
        "preflight",
    ]));
    assert_eq!(preflight["plan_id"], planned["plan_id"]);
    assert_eq!(package_names(&preflight), package_names(&planned));
    let log = fs::read_to_string(fixture.root().join("cargo.log")).expect("dry-run log");
    assert_eq!(
        log.lines().count(),
        2,
        "one verification for each eligible package"
    );
    assert!(
        log.lines()
            .all(|line| line.contains("--dry-run") && line.contains("--registry private-reg")),
        "{log}"
    );
    assert!(
        !fixture.root().join("markers").exists(),
        "preflight never uploads"
    );
}

fn execution_command(fixture: &Fixture, command: &str, attempts: &str) -> Command {
    let mut invocation = fixture.command();
    invocation.args([
        "--allow-loopback",
        "--registries",
        "crates-io,private-reg",
        "--verify-mode",
        "none",
        "--no-readiness",
        "--verify-timeout",
        "0ms",
        "--verify-poll",
        "0ms",
        "--base-delay",
        "0ms",
        "--max-attempts",
        attempts,
        command,
    ]);
    invocation
}

#[test]
fn publish_and_resume_use_each_targets_graph_and_id() {
    let fixture = Fixture::new();
    let public = LocalRegistry::new(&fixture, "crates-io");
    let private = LocalRegistry::new(&fixture, "private-reg");
    fixture.catalog(&public.api, &private.api);
    let public_plan =
        json(
            fixture
                .command()
                .args(["--allow-loopback", "--registries", "crates-io", "plan"]),
        );
    let private_plan =
        json(
            fixture
                .command()
                .args(["--allow-loopback", "--registries", "private-reg", "plan"]),
        );
    assert_eq!(package_names(&public_plan), ["common"]);
    assert_eq!(package_names(&private_plan), ["common", "private-only"]);
    assert_ne!(public_plan["plan_id"], private_plan["plan_id"]);
    public.assert_no_requests();
    private.assert_no_requests();

    let first = execution_command(&fixture, "publish", "1")
        .env("SHIPPER_TEST_FAIL_PRIVATE", "1")
        .assert()
        .failure()
        .get_output()
        .clone();
    let receipts: Vec<Value> = serde_json::Deserializer::from_slice(&first.stdout)
        .into_iter()
        .collect::<Result<_, _>>()
        .expect("publish JSON stream");
    assert_eq!(
        receipts.len(),
        1,
        "only the completed public target has a finalized receipt"
    );
    assert_eq!(receipts[0]["plan_id"], public_plan["plan_id"]);
    assert_eq!(package_names(&receipts[0]["receipt"]), ["common"]);
    let failure: Value = serde_json::from_slice(&first.stderr).expect("publish error envelope");
    assert_eq!(failure["schema_version"], "shipper.publish.error.v1");
    assert_eq!(failure["category"], "publish_failed");

    // A failed target retains its own plan and package state, without claiming
    // a completed receipt. This fixture exercises an ordinary retryable failure,
    // not the separate NotPublished reconciliation/controlled-stop protocol.
    let private_state: Value = serde_json::from_slice(
        &fs::read(fixture.root().join(".shipper/private-reg/state.json")).expect("private state"),
    )
    .expect("parse private state");
    assert_eq!(private_state["plan_id"], private_plan["plan_id"]);
    assert_eq!(private_state["registry"], private_plan["registry"]);
    assert_eq!(
        private_state["packages"]
            .as_object()
            .expect("state packages")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["common@0.1.0", "private-only@0.1.0"]
    );
    assert_eq!(
        private_state["packages"]["common@0.1.0"]["state"]["state"],
        "published"
    );
    assert_eq!(
        private_state["packages"]["private-only@0.1.0"]["state"]["state"],
        "failed"
    );
    assert_eq!(
        private_state["packages"]["private-only@0.1.0"]["state"]["class"],
        "retryable"
    );
    let events = fs::read_to_string(fixture.root().join(".shipper/private-reg/events.jsonl"))
        .expect("private events");
    assert!(
        events
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("event JSON"))
            .any(|event| {
                event["event_type"]["type"] == "plan_created"
                    && event["event_type"]["plan_id"] == private_plan["plan_id"]
                    && event["event_type"]["package_count"] == 2
            })
    );
    assert!(
        !fixture
            .root()
            .join(".shipper/private-reg/receipt.json")
            .exists()
    );
    assert!(
        !fixture
            .root()
            .join("markers/crates-io/private-only")
            .exists()
    );
    assert!(
        !fixture
            .root()
            .join("markers/private-reg/private-only")
            .exists()
    );

    let resumed = execution_command(&fixture, "resume", "2")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let receipts: Vec<Value> = serde_json::Deserializer::from_slice(&resumed)
        .into_iter()
        .collect::<Result<_, _>>()
        .expect("resume JSON stream");
    assert_eq!(receipts.len(), 2);
    for (index, expected) in [&public_plan, &private_plan].into_iter().enumerate() {
        assert_eq!(receipts[index]["plan_id"], expected["plan_id"]);
    }
    assert!(
        fixture
            .root()
            .join("markers/private-reg/private-only")
            .exists()
    );
    assert!(
        !fixture
            .root()
            .join("markers/crates-io/private-only")
            .exists()
    );
    let log = fs::read_to_string(fixture.root().join("cargo.log")).expect("publish log");
    assert_eq!(
        log.lines().count(),
        4,
        "successful packages must not be uploaded again: {log}"
    );
    assert_eq!(
        log.lines()
            .filter(|line| line.contains("private-only"))
            .count(),
        2
    );
    assert!(
        log.lines()
            .filter(|line| line.contains("private-only"))
            .all(|line| line.contains("--registry private-reg"))
    );
    assert_eq!(
        log.lines()
            .filter(|line| line.contains("-p common") && !line.contains("--registry"))
            .count(),
        1
    );
    assert_eq!(
        log.lines()
            .filter(|line| line.contains("-p common") && line.contains("--registry private-reg"))
            .count(),
        1
    );
}

#[test]
fn multiple_plan_or_preflight_targets_refuse_before_effects() {
    let fixture = Fixture::new();
    let public = LocalRegistry::new(&fixture, "crates-io");
    let private = LocalRegistry::new(&fixture, "private-reg");
    fixture.catalog(&public.api, &private.api);
    for command in ["plan", "preflight"] {
        for selector in [
            vec!["--registries", "crates-io,private-reg"],
            vec!["--all-registries"],
        ] {
            let output = fixture
                .command()
                .arg("--allow-loopback")
                .args(selector)
                .arg(command)
                .assert()
                .failure()
                .get_output()
                .clone();
            assert!(output.stdout.is_empty(), "no misleading single-target JSON");
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains("supports one registry at a time"),
                "{stderr}"
            );
            assert!(stderr.contains("--registries crates-io"), "{stderr}");
        }
    }
    fixture.assert_no_execution();
    public.assert_no_requests();
    private.assert_no_requests();
}

#[test]
fn invalid_later_target_and_unsafe_config_refuse_before_effects() {
    let fixture = Fixture::new();
    let public = LocalRegistry::new(&fixture, "crates-io");
    let private = LocalRegistry::new(&fixture, "private-reg");
    fixture.catalog(&public.api, &private.api);
    let output = fixture
        .command()
        .args([
            "--allow-loopback",
            "--registries",
            "private-reg,crates-io",
            "--package",
            "private-only",
            "publish",
        ])
        .assert()
        .failure()
        .get_output()
        .clone();
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).expect("planning error envelope");
    assert_eq!(error["schema_version"], "shipper.publish.error.v1");
    assert_eq!(
        error["category"], "publish_failed",
        "target context must not reclassify a selection failure as registry connectivity"
    );
    assert_eq!(error["evidence"], serde_json::json!([]));
    fixture.assert_no_execution();
    public.assert_no_requests();
    private.assert_no_requests();

    let output = fixture
        .command_with_format("text")
        .args([
            "--allow-loopback",
            "--registries",
            "private-reg,crates-io",
            "--package",
            "private-only",
            "publish",
        ])
        .assert()
        .failure()
        .get_output()
        .clone();
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to build plan for registry 'crates-io'"),
        "{stderr}"
    );
    fixture.assert_no_execution();
    public.assert_no_requests();
    private.assert_no_requests();

    fixture.config(&private.api);
    let output = fixture
        .command()
        .args(["--package", "private-only", "publish"])
        .assert()
        .failure()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Configuration validation failed"),
        "{stderr}"
    );
    fixture.assert_no_execution();
    private.assert_no_requests();
}
