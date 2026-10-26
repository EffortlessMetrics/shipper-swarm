//! Receipt source evidence follows the selected workspace, never the caller CWD.

use std::path::PathBuf;
use std::process::Command as StdCommand;

use serde_json::{Value, json};
use tempfile::{TempDir, tempdir_in};

use super::*;

fn git(root: &Path, args: &[&str]) -> String {
    let output = StdCommand::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("run fixture Git command");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("Git output is UTF-8")
        .trim()
        .to_owned()
}

fn init_git(root: &Path, branch: &str) -> String {
    git(root, &["init", "-q", "-b", branch]);
    git(root, &["config", "user.name", "Receipt Fixture"]);
    git(root, &["config", "user.email", "receipt@example.invalid"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-q", "-m", branch],
    );
    git(
        root,
        &["-c", "tag.gpgSign=false", "tag", &format!("{branch}-tag")],
    );
    git(root, &["rev-parse", "HEAD"])
}

struct Fixture {
    root: TempDir,
    target: PathBuf,
    caller: PathBuf,
    commit: String,
    new_path: String,
    real_cargo: String,
    fake_cargo: String,
}

impl Fixture {
    fn new() -> Self {
        let root = tempdir().expect("fixture directory");
        let target = root.path().join("target-workspace");
        create_single_crate_workspace(&target);
        write_file(&target.join(".gitignore"), "Cargo.lock\n/target/\n");
        let commit = init_git(&target, "target");

        let caller = root.path().join("caller-repository");
        write_file(&caller.join("README"), "a different repository\n");
        let caller_commit = init_git(&caller, "caller");
        assert_ne!(commit, caller_commit, "distinct source identities");
        write_file(&caller.join("untracked"), "caller is dirty\n");

        let (new_path, real_cargo, fake_cargo) = setup_fake_cargo(root.path());
        Self {
            root,
            target,
            caller,
            commit,
            new_path,
            real_cargo,
            fake_cargo,
        }
    }

    fn publish_and_resume(
        &self,
        target: &Path,
        caller: &Path,
        flags: &[&str],
        git_override: Option<&Path>,
    ) -> Result<Vec<Value>> {
        let state = tempdir_in(self.root.path())?;
        let publish_log = state.path().join("cargo-publish.log");
        // The first process sees the version already published. The second
        // trusts its terminal state; neither may invoke Cargo publication.
        let registry = spawn_bounded_registry(vec![200], 1);
        let result = (|| -> Result<Vec<Value>> {
            let mut contexts = Vec::new();
            for operation in ["publish", "resume"] {
                let mut command = loopback_shipper_cmd();
                command
                    .timeout(Duration::from_secs(15))
                    .current_dir(caller)
                    .arg("--manifest-path")
                    .arg(target.join("Cargo.toml"))
                    .arg("--api-base")
                    .arg(&registry.base_url)
                    .arg("--state-dir")
                    .arg(state.path())
                    .args([
                        "--policy",
                        "fast",
                        "--verify-mode",
                        "none",
                        "--no-readiness",
                        "--skip-ownership-check",
                    ])
                    .args(flags)
                    .arg(operation)
                    .env("PATH", &self.new_path)
                    .env("REAL_CARGO", &self.real_cargo)
                    .env("CARGO_NET_OFFLINE", "true")
                    .env("SHIPPER_CARGO_BIN", &self.fake_cargo)
                    .env("SHIPPER_FAKE_PUBLISH_LOG", &publish_log)
                    .env("SHIPPER_FAKE_PUBLISH_EXIT", "71")
                    .env("SHIPPER_TEST_GIT_LOG", self.root.path().join("git-calls"))
                    .env_remove("SHIPPER_GIT_BIN");
                if let Some(program) = git_override {
                    command.env("SHIPPER_GIT_BIN", program);
                }
                let output = command.output()?;
                ensure!(
                    output.status.success(),
                    "{operation} from {}: {}",
                    caller.display(),
                    String::from_utf8_lossy(&output.stderr)
                );
                let receipt: Value =
                    serde_json::from_slice(&fs::read(state.path().join("receipt.json"))?)?;
                ensure!(receipt["execution_result"] == "success");
                contexts.push(receipt["git_context"].clone());
            }
            Ok(contexts)
        })();
        // Join even when the subprocess or receipt assertions failed.
        let registry_result = registry.finish(Duration::from_secs(2));
        let contexts = result?;
        registry_result?;
        ensure!(
            read_publish_log(&publish_log).is_empty(),
            "already-published fixture invoked Cargo publication"
        );
        Ok(contexts)
    }

    fn expected_context(&self, branch: Option<&str>, dirty: bool) -> Value {
        json!({
            "commit": self.commit,
            "branch": branch,
            "tag": "target-tag",
            "dirty": dirty,
        })
    }
}

#[test]
#[serial_test::serial]
fn selected_workspace_receipt_identity_survives_caller_and_scheduler_changes() -> Result<()> {
    let fixture = Fixture::new();
    let outside = fixture.root.path().join("not-a-repository");
    fs::create_dir(&outside)?;
    let subdirectory = fixture.target.join("demo/src");
    let expected = fixture.expected_context(Some("target"), false);

    for caller in [&fixture.target, &subdirectory, &fixture.caller, &outside] {
        for flags in [&[][..], &["--parallel"][..]] {
            for actual in fixture.publish_and_resume(&fixture.target, caller, flags, None)? {
                assert_eq!(
                    actual,
                    expected,
                    "caller={}, flags={flags:?}",
                    caller.display()
                );
            }
        }
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn selected_workspace_receipt_supports_detached_dirty_and_linked_worktrees() -> Result<()> {
    let fixture = Fixture::new();
    git(&fixture.target, &["checkout", "--detach", "-q"]);
    write_file(
        &fixture.target.join("demo/src/lib.rs"),
        "pub fn changed() {}\n",
    );
    for actual in
        fixture.publish_and_resume(&fixture.target, &fixture.caller, &["--allow-dirty"], None)?
    {
        assert_eq!(actual, fixture.expected_context(None, true));
    }

    let linked = fixture.root.path().join("linked-worktree");
    git(
        &fixture.target,
        &[
            "worktree",
            "add",
            "--detach",
            linked.to_str().context("linked path UTF-8")?,
            "HEAD",
        ],
    );
    for actual in fixture.publish_and_resume(&linked, &fixture.caller, &["--parallel"], None)? {
        assert_eq!(actual, fixture.expected_context(None, false));
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn unavailable_target_git_evidence_never_borrows_the_caller_repository() -> Result<()> {
    let fixture = Fixture::new();
    let missing_git = fixture.root.path().join("missing-git-executable");
    for actual in fixture.publish_and_resume(
        &fixture.target,
        &fixture.caller,
        &["--allow-dirty"],
        Some(&missing_git),
    )? {
        assert!(actual.is_null(), "failed override must not use real Git");
    }

    fs::remove_dir_all(fixture.target.join(".git"))?;
    for actual in fixture.publish_and_resume(
        &fixture.target,
        &fixture.caller,
        &["--allow-dirty", "--parallel"],
        None,
    )? {
        assert!(actual.is_null(), "non-Git target must not use caller Git");
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn receipt_git_override_queries_the_selected_workspace() -> Result<()> {
    let fixture = Fixture::new();
    #[cfg(windows)]
    let program = {
        let path = fixture.root.path().join("git-proxy.cmd");
        fs::write(
            &path,
            "@echo off\r\necho %CD%>>\"%SHIPPER_TEST_GIT_LOG%\"\r\ngit %*\r\nexit /b %ERRORLEVEL%\r\n",
        )?;
        path
    };
    #[cfg(not(windows))]
    let program = {
        use std::os::unix::fs::PermissionsExt;
        let path = fixture.root.path().join("git-proxy");
        fs::write(
            &path,
            "#!/bin/sh\npwd >> \"$SHIPPER_TEST_GIT_LOG\"\nexec git \"$@\"\n",
        )?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        path
    };
    for actual in fixture.publish_and_resume(
        &fixture.target,
        &fixture.caller,
        &["--parallel"],
        Some(&program),
    )? {
        assert_eq!(actual, fixture.expected_context(Some("target"), false));
    }
    let calls = fs::read_to_string(fixture.root.path().join("git-calls"))?;
    ensure!(!calls.is_empty(), "Git override must be invoked");
    let expected_root = fixture.target.canonicalize()?;
    for caller in calls.lines() {
        assert_eq!(Path::new(caller).canonicalize()?, expected_root);
    }
    Ok(())
}
