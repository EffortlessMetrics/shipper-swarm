"""Nonpublishing regression proof for the actual release binary workflow steps.

A tiny native Rust protocol fixture tests the smoke helper's refusal paths.
Cross-platform aggregate receipts/headers are explicitly synthetic protocol
fixtures, not evidence that Shipper ran on those platforms. Release qualification
must execute each actual Shipper archive on its matching native runner.
"""

import argparse
import copy
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/release.yml"
VERSION = "0.5.0"
TARGETS = {
    "x86_64-unknown-linux-gnu": ("tar.gz", "Linux", "x86_64", "shipper"),
    "x86_64-apple-darwin": ("tar.gz", "Darwin", "x86_64", "shipper"),
    "aarch64-apple-darwin": ("tar.gz", "Darwin", "aarch64", "shipper"),
    "x86_64-pc-windows-msvc": ("zip", "Windows", "x86_64", "shipper.exe"),
}


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def checked(args, *, cwd, env):
    return subprocess.run(args, cwd=cwd, env=env, capture_output=True, text=True, check=True, timeout=45).stdout.strip()


def workflow_step(name):
    text = WORKFLOW.read_text(encoding="utf-8")
    match = re.search(r"^      - name: " + re.escape(name) + r"\n(?P<body>.*?)(?=^      - |^  [a-z]|\Z)", text, re.M | re.S)
    if match is None:
        raise AssertionError(f"missing workflow step: {name}")
    block = re.search(r"^        run: \|\n(?P<script>(?:^          .*\n|^\n)+)", match["body"], re.M)
    if block is None:
        raise AssertionError(f"missing Bash run block: {name}")
    return "\n".join(line[10:] if line.startswith("          ") else line for line in block["script"].splitlines())


def native_fixture_source():
    # This program deliberately implements only a synthetic probe protocol. It
    # is never presented as the production CLI or a native target qualification.
    return r'''
use std::{env, path::Path, process};
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let case = env::var("SMOKE_FIXTURE_CASE").unwrap_or_default();
    if args == ["--version"] {
        println!("shipper {}", if case == "wrong-version" { "0.0.1" } else { "0.5.0" });
    } else if args == ["--help"] {
        println!("{}", if case == "bad-help" { "incomplete" } else { "Usage: shipper plan publish resume" });
    } else if args.iter().any(|arg| arg == "plan") {
        assert_eq!(env::var("CARGO_NET_OFFLINE").unwrap(), "true");
        assert_eq!(std::fs::read_dir(env::var("CARGO_HOME").unwrap()).unwrap().count(), 0);
        let manifest = &args[args.iter().position(|arg| arg == "--manifest-path").unwrap() + 1];
        let root = Path::new(manifest).parent().unwrap().display().to_string();
        let name = if case == "wrong-package" { "unexpected-package" } else { "shipper-packaged-smoke-fixture" };
        let registry = if case == "wrong-registry" { "other-registry" } else { "crates-io" };
        let published = case == "published";
        println!(r#"{{"schema_version":"shipper.plan.v1","outcome":{{"status":"planned","publication_performed":{published}}},"plan_id":"{}","workspace_root":{root:?},"publishable_count":1,"skipped_count":0,"packages":[{{"name":{name:?},"version":"0.0.0","dependencies":[],"manifest_path":{manifest:?}}}],"registry":{{"name":{registry:?},"api_base":"https://crates.io"}}}}"#, "a".repeat(64));
    } else {
        process::exit(99);
    }
}
'''


def synthetic_header(target):
    data = bytearray(128)
    if target.endswith("linux-gnu"):
        data[:6] = b"\x7fELF\x02\x01"
        struct.pack_into("<H", data, 18, 62)
    elif target.endswith("apple-darwin"):
        data[:4] = b"\xcf\xfa\xed\xfe"
        struct.pack_into("<I", data, 4, 0x0100000C if target.startswith("aarch64") else 0x01000007)
    else:
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 60, 64)
        data[64:68] = b"PE\0\0"
        struct.pack_into("<H", data, 68, 0x8664)
        struct.pack_into("<H", data, 88, 0x20B)
    return bytes(data)


def make_archive(path, target, data, *, mode=0o755, member=None, symlink=False):
    path.parent.mkdir(parents=True, exist_ok=True)
    name = member or TARGETS[target][3]
    if TARGETS[target][0] == "zip":
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr(name, data)
    else:
        with tarfile.open(path, "w:gz") as archive:
            entry = tarfile.TarInfo(name)
            entry.mode = mode
            entry.size = len(data)
            if symlink:
                entry.type = tarfile.SYMTYPE
                entry.linkname = "/bin/true"
                entry.size = 0
            archive.addfile(entry, None if symlink else io.BytesIO(data))


class BinaryProof(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.storage = tempfile.TemporaryDirectory(prefix="shipper-binary-proof-tests-")
        cls.repo = Path(cls.storage.name)
        cls.env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        cls.env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        checked(["git", "init", "--quiet"], cwd=cls.repo, env=cls.env)
        for key, value in {"user.name": "Release proof fixture", "user.email": "fixture@example.invalid",
                           "commit.gpgsign": "false", "tag.gpgsign": "false"}.items():
            checked(["git", "config", key, value], cwd=cls.repo, env=cls.env)
        (cls.repo / "Cargo.lock").write_text("version = 4\n", encoding="utf-8")
        helper = cls.repo / "scripts/release/verify_binary.py"
        helper.parent.mkdir(parents=True)
        shutil.copyfile(ROOT / "scripts/release/verify_binary.py", helper)
        checked(["git", "add", "Cargo.lock", "scripts"], cwd=cls.repo, env=cls.env)
        checked(["git", "commit", "--quiet", "-m", "bounded source fixture"], cwd=cls.repo, env=cls.env)
        cls.sha = checked(["git", "rev-parse", "HEAD"], cwd=cls.repo, env=cls.env)
        cls.tree = checked(["git", "rev-parse", "HEAD^{tree}"], cwd=cls.repo, env=cls.env)
        cls.env.update(APPROVED_SHA=cls.sha, APPROVED_TREE=cls.tree, APPROVED_VERSION=VERSION)
        cls.lock_digest = sha256((cls.repo / "Cargo.lock").read_bytes())
        rustc = checked(["rustc", "-vV"], cwd=cls.repo, env=cls.env)
        cls.native_target = next(line.removeprefix("host: ") for line in rustc.splitlines() if line.startswith("host: "))
        if cls.native_target not in TARGETS:
            raise RuntimeError(f"native fixture requires a release target host, observed {cls.native_target}")
        source = cls.repo / "protocol-fixture.rs"
        source.write_text(native_fixture_source(), encoding="utf-8")
        executable = cls.repo / TARGETS[cls.native_target][3]
        checked(["rustc", "--edition=2021", str(source), "-o", str(executable)], cwd=cls.repo, env=cls.env)
        cls.native_data = executable.read_bytes()

    @classmethod
    def tearDownClass(cls):
        cls.storage.cleanup()

    def setUp(self):
        self.artifacts = self.repo / "artifacts"
        shutil.rmtree(self.artifacts, ignore_errors=True)
        self.output = self.repo / "result.json"
        self.output.unlink(missing_ok=True)
        for path in self.repo.glob("shipper-*"):
            if path.is_file():
                path.unlink()

    def archive_path(self, target, *, aggregate=False):
        directory = self.artifacts / f"shipper-{target}" if aggregate else self.repo
        return directory / f"shipper-{target}.{TARGETS[target][0]}"

    def run_helper(self, mode, *args, env=None):
        return subprocess.run([sys.executable, "scripts/release/verify_binary.py", mode,
                               "--approved-sha", self.sha, "--approved-tree", self.tree,
                               "--approved-version", VERSION, "--output", str(self.output), *args],
                              cwd=self.repo, env=env or self.env, capture_output=True, text=True, timeout=60)

    def smoke(self, data=None, *, env=None, **archive_options):
        archive = self.archive_path(self.native_target)
        make_archive(archive, self.native_target, self.native_data if data is None else data, **archive_options)
        return self.run_helper("smoke", "--target", self.native_target, "--archive", str(archive), env=env)

    def assert_pass(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def assert_refused(self, result, diagnostic):
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(diagnostic, result.stderr)
        self.assertFalse(self.output.exists(), "failed proof must not emit a success receipt")

    def receipt(self, target, archive, data):
        return {
            "schema_version": "shipper.release_binary_smoke.v1", "result": "success", "target": target,
            "source_sha": self.sha, "source_tree": self.tree, "approved_version": VERSION,
            "lockfile_sha256": self.lock_digest, "artifact": archive.name,
            "archive_sha256": sha256(archive.read_bytes()), "executable_sha256": sha256(data),
            "host": {"system": TARGETS[target][1], "machine": TARGETS[target][2]},
            "toolchain": {"rustc_verbose": f"rustc synthetic fixture\nhost: {target}", "cargo": "cargo synthetic fixture"},
            "checks": {
                "version": {"result": "success", "exit_code": 0, "stdout": f"shipper {VERSION}"},
                "help": {"result": "success", "exit_code": 0, "stdout_sha256": "a" * 64},
                "plan": {"result": "success", "exit_code": 0, "cargo_offline": True, "schema_version": "shipper.plan.v1",
                         "publication_performed": False, "package": {"name": "shipper-packaged-smoke-fixture", "version": "0.0.0", "dependencies": []},
                         "registry": {"name": "crates-io", "api_base": "https://crates.io"}, "plan_id": "b" * 64, "stdout_sha256": "c" * 64},
            },
        }

    def write_receipt(self, archive, receipt):
        path = archive.with_name(archive.name + ".smoke.json")
        path.write_text(json.dumps(receipt), encoding="utf-8")
        return path

    def aggregate_fixtures(self, *, plaintext=False):
        result = {}
        for target in TARGETS:
            archive = self.archive_path(target, aggregate=True)
            data = b"ordinary text, not a release executable\n" if plaintext else synthetic_header(target)
            if plaintext:
                archive.parent.mkdir(parents=True)
                archive.write_bytes(data)
            else:
                make_archive(archive, target, data)
            receipt = self.receipt(target, archive, data)
            self.write_receipt(archive, receipt)
            metadata = {"schema_version": "shipper.release_binary.v1", "source_sha": self.sha, "source_tree": self.tree,
                        "target": target, "artifact": archive.name, "runner": "synthetic fixture", "retention_days": 7}
            archive.with_name(archive.name + ".metadata.json").write_text(json.dumps(metadata), encoding="utf-8")
            archive.with_name(archive.name + ".sha256").write_text(f"{sha256(archive.read_bytes())}  {archive.name}\n", encoding="utf-8")
            result[target] = (archive, receipt)
        return result

    def aggregate(self):
        return self.run_helper("aggregate", "--artifacts", str(self.artifacts))

    def run_workflow(self, name):
        script = workflow_step(name).replace("${{ matrix.target }}", self.native_target).replace("${{ matrix.archive }}", TARGETS[self.native_target][0])
        return subprocess.run(["bash", "-euo", "pipefail", "-c", script], cwd=self.repo, env=self.env,
                              capture_output=True, text=True, timeout=60)

    def test_native_archive_executes_version_help_and_offline_plan(self):
        self.assert_pass(self.smoke())
        receipt = json.loads(self.output.read_text())
        self.assertEqual(receipt["source_sha"], self.sha)
        self.assertEqual(receipt["source_tree"], self.tree)
        self.assertEqual(receipt["executable_sha256"], sha256(self.native_data))
        self.assertEqual(set(receipt["checks"]), {"version", "help", "plan"})
        self.assertTrue(receipt["checks"]["plan"]["cargo_offline"])

    def test_actual_native_workflow_step_produces_retained_receipt(self):
        archive = self.archive_path(self.native_target)
        make_archive(archive, self.native_target, self.native_data)
        self.assert_pass(self.run_workflow("Smoke packaged release executable"))
        receipt = json.loads(archive.with_name(archive.name + ".smoke.json").read_text())
        self.assertEqual(receipt["checks"]["plan"]["package"]["name"], "shipper-packaged-smoke-fixture")

    def test_native_wrong_version_is_rejected(self):
        self.assert_refused(self.smoke(env={**self.env, "SMOKE_FIXTURE_CASE": "wrong-version"}), "packaged version mismatch")

    def test_native_missing_help_is_rejected(self):
        self.assert_refused(self.smoke(env={**self.env, "SMOKE_FIXTURE_CASE": "bad-help"}), "help is missing")

    def test_native_incorrect_plan_is_rejected(self):
        for case, message in [("wrong-package", "package name"), ("wrong-registry", "registry metadata"), ("published", "nonpublishing plan")]:
            with self.subTest(case=case):
                self.assert_refused(self.smoke(env={**self.env, "SMOKE_FIXTURE_CASE": case}), message)

    def test_missing_archive_is_rejected(self):
        self.assert_refused(self.run_helper("smoke", "--target", self.native_target, "--archive", str(self.archive_path(self.native_target))), "No such file")

    def test_plaintext_payload_is_rejected(self):
        self.assert_refused(self.smoke(b"not an executable"), "not a native executable")

    def test_missing_archive_member_is_rejected(self):
        self.assert_refused(self.smoke(member="unrelated"), "exactly one regular")

    def test_archive_path_traversal_is_rejected(self):
        self.assert_refused(self.smoke(member="../shipper"), "exactly one regular")

    @unittest.skipIf(platform.system() == "Windows", "Unix archive permission contract")
    def test_nonexecutable_member_is_rejected(self):
        self.assert_refused(self.smoke(mode=0o644), "executable permission")

    @unittest.skipIf(platform.system() == "Windows", "Unix archive member type contract")
    def test_symlink_member_is_rejected(self):
        self.assert_refused(self.smoke(symlink=True), "exactly one regular")

    def test_wrong_architecture_is_rejected(self):
        data = bytearray(self.native_data)
        if self.native_target.endswith("linux-gnu"):
            struct.pack_into("<H", data, 18, 183)
        elif self.native_target.endswith("apple-darwin"):
            struct.pack_into("<I", data, 4, 0)
        else:
            struct.pack_into("<H", data, struct.unpack_from("<I", data, 60)[0] + 4, 0xAA64)
        self.assert_refused(self.smoke(data), "wrong architecture")

    def test_wrong_native_host_is_rejected(self):
        target = next(target for target in TARGETS if TARGETS[target][1] != TARGETS[self.native_target][1])
        archive = self.archive_path(target)
        make_archive(archive, target, synthetic_header(target))
        self.assert_refused(self.run_helper("smoke", "--target", target, "--archive", str(archive)), "requires native")

    def test_changed_lockfile_is_rejected(self):
        path = self.repo / "Cargo.lock"
        original = path.read_bytes()
        try:
            path.write_text("version = 3\n", encoding="utf-8")
            self.assert_refused(self.smoke(), "immutability check failed")
        finally:
            path.write_bytes(original)

    def test_aggregate_protocol_accepts_complete_matching_receipts(self):
        self.aggregate_fixtures()
        self.assert_pass(self.aggregate())
        self.assertEqual({item["target"] for item in json.loads(self.output.read_text())["targets"]}, set(TARGETS))

    def test_actual_aggregate_workflow_accepts_complete_protocol_fixture(self):
        self.aggregate_fixtures()
        self.assert_pass(self.run_workflow("Verify binary identity and checksums"))

    def test_aggregate_requires_every_native_receipt(self):
        fixtures = self.aggregate_fixtures()
        for target, (archive, receipt) in fixtures.items():
            with self.subTest(target=target):
                archive.with_name(archive.name + ".smoke.json").unlink()
                self.assert_refused(self.aggregate(), "missing native executable smoke receipt")
                self.write_receipt(archive, receipt)

    def test_aggregate_rejects_wrong_identity_version_target_and_digest(self):
        archive, receipt = next(iter(self.aggregate_fixtures().values()))
        for field in ["source_sha", "source_tree", "approved_version", "target", "lockfile_sha256", "archive_sha256", "executable_sha256"]:
            with self.subTest(field=field):
                changed = copy.deepcopy(receipt)
                changed[field] = "wrong"
                self.write_receipt(archive, changed)
                self.assert_refused(self.aggregate(), "mismatch")

    def test_aggregate_rejects_skipped_missing_and_failed_checks(self):
        archive, receipt = next(iter(self.aggregate_fixtures().values()))
        for name in ["version", "help", "plan"]:
            for state in ["skipped", "failed", "missing"]:
                with self.subTest(check=name, state=state):
                    changed = copy.deepcopy(receipt)
                    if state == "missing":
                        del changed["checks"][name]
                    else:
                        changed["checks"][name]["result"] = state
                    self.write_receipt(archive, changed)
                    self.assert_refused(self.aggregate(), "proof")

    def test_aggregate_rejects_foreign_host_or_missing_toolchain(self):
        archive, receipt = next(iter(self.aggregate_fixtures().values()))
        for field, value, message in [("host", {"system": "other", "machine": "other"}, "host evidence"),
                                      ("toolchain", {}, "toolchain evidence")]:
            with self.subTest(field=field):
                changed = copy.deepcopy(receipt)
                changed[field] = value
                self.write_receipt(archive, changed)
                self.assert_refused(self.aggregate(), message)

    def test_aggregate_rejects_tampered_archive_even_with_updated_checksum(self):
        archive, _ = next(iter(self.aggregate_fixtures().values()))
        archive.write_bytes(archive.read_bytes() + b"tamper")
        archive.with_name(archive.name + ".sha256").write_text(f"{sha256(archive.read_bytes())}  {archive.name}\n", encoding="utf-8")
        result = self.run_workflow("Verify binary identity and checksums")
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("archive_sha256 mismatch", result.stderr)

    def test_workflow_rejects_plaintext_even_with_consistent_checksums(self):
        self.aggregate_fixtures(plaintext=True)
        result = self.run_workflow("Verify binary identity and checksums")
        self.assertNotEqual(result.returncode, 0, "checksum-only verification accepted four ordinary text files:\n" + result.stdout)
        self.assertIn("invalid release archive", result.stderr)

    def test_workflow_requires_locked_build_and_native_smoke_before_upload(self):
        text = WORKFLOW.read_text(encoding="utf-8")
        build = text.split("  build-binaries:", 1)[1].split("  verify-binaries:", 1)[0]
        self.assertIn("cargo build --locked --release --target", build)
        self.assertLess(build.index("name: Smoke packaged release executable"), build.index("name: Upload artifact"))
        self.assertNotIn("continue-on-error", build)
        self.assertIn("artifacts/**/shipper-*.smoke.json", text)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workflow", type=Path, default=WORKFLOW)
    args, tests = parser.parse_known_args()
    WORKFLOW = args.workflow.resolve()
    unittest.main(argv=[sys.argv[0], *tests], verbosity=2)
