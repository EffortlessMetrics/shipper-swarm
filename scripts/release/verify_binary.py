"""Verify packaged Shipper executables and their retained native smoke receipts.

Smoke mode runs only the archive's extracted executable on its matching host.
Aggregate mode consumes those native receipts; it does not emulate other hosts.
No network, registry operation, credentials, or signing is involved.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import zipfile


TARGETS = {
    "x86_64-unknown-linux-gnu": ("tar.gz", "Linux", "x86_64", "shipper"),
    "x86_64-apple-darwin": ("tar.gz", "Darwin", "x86_64", "shipper"),
    "aarch64-apple-darwin": ("tar.gz", "Darwin", "aarch64", "shipper"),
    "x86_64-pc-windows-msvc": ("zip", "Windows", "x86_64", "shipper.exe"),
}
SCHEMA = "shipper.release_binary_smoke.v1"
MAX_PAYLOAD = 256 * 1024 * 1024
SMOKE_PACKAGE = {"name": "shipper-packaged-smoke-fixture", "version": "0.0.0", "dependencies": []}
SMOKE_REGISTRY = {"name": "crates-io", "api_base": "https://crates.io"}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def sha256_file(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def command(args, label, *, cwd=None, env=None):
    try:
        result = subprocess.run(args, cwd=cwd, env=env, capture_output=True, text=True, timeout=20)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ValueError(f"{label} could not complete: {error}") from error
    require(result.returncode == 0, f"{label} failed (exit {result.returncode}): {result.stderr[:500]}")
    return result.stdout.strip()


def validate_approval(sha, tree, version):
    require(re.fullmatch(r"[0-9a-f]{40}", sha) is not None, "approved source SHA must be 40 lowercase hex characters")
    require(re.fullmatch(r"[0-9a-f]{40}", tree) is not None, "approved source tree must be 40 lowercase hex characters")
    require(re.fullmatch(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.+-]+)?", version) is not None, "approved version must be an exact release version")


def source_identity(sha, tree):
    require(command(["git", "rev-parse", "HEAD"], "source commit read") == sha, "checked-out source SHA differs from approval")
    require(command(["git", "rev-parse", "HEAD^{tree}"], "source tree read") == tree, "checked-out source tree differs from approval")
    command(["git", "diff", "--exit-code", "HEAD", "--"], "tracked source/Cargo.lock immutability check")
    lock = Path("Cargo.lock")
    require(lock.is_file(), "approved checkout is missing Cargo.lock")
    return sha256_file(lock)


def canonical_archive(target):
    return f"shipper-{target}.{TARGETS[target][0]}"


def host_identity(target):
    expected_system, expected_machine = TARGETS[target][1:3]
    system = platform.system()
    raw_machine = platform.machine()
    machine = {"AMD64": "x86_64", "amd64": "x86_64", "arm64": "aarch64"}.get(raw_machine, raw_machine)
    require((system, machine) == (expected_system, expected_machine),
            f"target {target} requires native {expected_system}/{expected_machine}; observed {system}/{raw_machine}")
    return {"system": system, "machine": machine}


def check_executable_header(data, target):
    """Reject renamed text and mismatched architectures before native execution."""
    require(len(data) >= 64, "packaged executable is too short or is not a native executable")
    if target.endswith("linux-gnu"):
        require(data[:6] == b"\x7fELF\x02\x01", "packaged executable is not a 64-bit little-endian ELF")
        require(struct.unpack_from("<H", data, 18)[0] == 62, "packaged ELF has the wrong architecture; expected x86_64")
    elif target.endswith("apple-darwin"):
        require(data[:4] == b"\xcf\xfa\xed\xfe", "packaged executable is not a thin 64-bit Mach-O")
        expected_cpu = 0x0100000C if target.startswith("aarch64") else 0x01000007
        require(struct.unpack_from("<I", data, 4)[0] == expected_cpu, "packaged Mach-O has the wrong architecture")
    else:
        require(data[:2] == b"MZ", "packaged executable is not a Windows PE image")
        offset = struct.unpack_from("<I", data, 60)[0]
        require(offset + 26 <= len(data) and data[offset:offset + 4] == b"PE\0\0", "packaged executable has an invalid PE header")
        require(struct.unpack_from("<H", data, offset + 4)[0] == 0x8664, "packaged PE has the wrong architecture; expected x86_64")
        require(struct.unpack_from("<H", data, offset + 24)[0] == 0x20B, "packaged PE is not a 64-bit executable")


def archive_payload(archive, target):
    require(archive.name == canonical_archive(target), f"unexpected archive name for {target}: {archive.name}")
    expected_name = TARGETS[target][3]
    try:
        if TARGETS[target][0] == "zip":
            with zipfile.ZipFile(archive) as package:
                entries = package.infolist()
                require(len(entries) == 1 and entries[0].filename == expected_name and not entries[0].is_dir(),
                        f"archive must contain exactly one regular {expected_name}")
                entry = entries[0]
                stored_mode = entry.external_attr >> 16
                require(stat.S_IFMT(stored_mode) in (0, stat.S_IFREG), "archive executable must not be a link or special file")
                require(0 < entry.file_size <= MAX_PAYLOAD, "archive executable has an invalid or excessive size")
                with package.open(entry) as stream:
                    data = stream.read(MAX_PAYLOAD + 1)
                mode = 0o755
        else:
            with tarfile.open(archive, "r:gz") as package:
                entries = package.getmembers()
                require(len(entries) == 1 and entries[0].name == expected_name and entries[0].isfile(),
                        f"archive must contain exactly one regular {expected_name}")
                entry = entries[0]
                require(entry.mode & 0o111, "packaged executable does not have executable permission")
                require(0 < entry.size <= MAX_PAYLOAD, "archive executable has an invalid or excessive size")
                with package.extractfile(entry) as stream:
                    data = stream.read(MAX_PAYLOAD + 1)
                mode = entry.mode & 0o777
    except (tarfile.TarError, zipfile.BadZipFile, OSError, RuntimeError) as error:
        raise ValueError(f"invalid release archive {archive.name}: {error}") from error
    require(0 < len(data) <= MAX_PAYLOAD, "archive executable has an invalid or excessive size")
    check_executable_header(data, target)
    return data, mode


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def offline_plan(executable, scratch):
    """Exercise real planning/Cargo metadata without dependencies or a registry call."""
    workspace = scratch / "workspace"
    (workspace / "src").mkdir(parents=True)
    manifest = workspace / "Cargo.toml"
    manifest.write_text('[package]\nname = "shipper-packaged-smoke-fixture"\nversion = "0.0.0"\nedition = "2021"\n\n[workspace]\n', encoding="utf-8")
    (workspace / "src" / "lib.rs").write_text("pub fn packaged_smoke() {}\n", encoding="utf-8")
    config = workspace / ".shipper.toml"
    config.write_text("", encoding="utf-8")
    cargo_home = scratch / "cargo-home"
    cargo_home.mkdir()
    # A fresh Cargo home has no registry credentials or config. Explicit CLI
    # selection and an empty Shipper config prevent the caller's configuration
    # from changing this read-only plan. Cargo must not refresh any index.
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(("CARGO_", "SHIPPER_")) and key != "CARGO"}
    env.update(CARGO_HOME=str(cargo_home), CARGO_NET_OFFLINE="true", CARGO_TARGET_DIR=str(scratch / "target"))
    output = command([str(executable), "--config", str(config), "--manifest-path", str(manifest),
                      "--registry", "crates-io", "--api-base", "https://crates.io", "plan", "--format", "json"],
                     "archive-extracted offline plan", cwd=workspace, env=env)
    report = json.loads(output)
    require(isinstance(report, dict) and report.get("schema_version") == "shipper.plan.v1", "offline plan has an invalid schema")
    require(isinstance(report.get("outcome"), dict) and report["outcome"].get("status") == "planned"
            and report["outcome"].get("publication_performed") is False, "offline plan did not report a nonpublishing plan")
    require(report.get("publishable_count") == 1 and report.get("skipped_count") == 0,
            "offline plan did not select the one expected package")
    packages = report.get("packages")
    require(isinstance(packages, list) and len(packages) == 1 and isinstance(packages[0], dict),
            "offline plan package metadata is missing")
    observed_package = {key: packages[0].get(key) for key in SMOKE_PACKAGE}
    require(observed_package == SMOKE_PACKAGE, "offline plan package name, version, or dependencies mismatch")
    registry = report.get("registry")
    require(isinstance(registry, dict) and {key: registry.get(key) for key in SMOKE_REGISTRY} == SMOKE_REGISTRY,
            "offline plan registry metadata mismatch")
    require(re.fullmatch(r"[0-9a-f]{64}", str(report.get("plan_id"))) is not None, "offline plan ID is missing or invalid")
    require(Path(report.get("workspace_root", "")).resolve() == workspace.resolve(), "offline plan used the wrong workspace")
    require(Path(packages[0].get("manifest_path", "")).resolve() == manifest.resolve(), "offline plan used the wrong manifest")
    return {"result": "success", "exit_code": 0, "cargo_offline": True, "schema_version": "shipper.plan.v1",
            "publication_performed": False, "package": observed_package, "registry": SMOKE_REGISTRY,
            "plan_id": report["plan_id"], "stdout_sha256": digest(output.encode())}


def smoke(archive, target, sha, tree, version, output):
    validate_approval(sha, tree, version)
    host = host_identity(target)
    lock_digest = source_identity(sha, tree)
    archive_digest = sha256_file(archive)
    data, mode = archive_payload(archive, target)
    with tempfile.TemporaryDirectory(prefix="shipper-packaged-smoke-") as scratch:
        executable = Path(scratch) / TARGETS[target][3]
        executable.write_bytes(data)
        executable.chmod(mode)
        # Absolute extracted path: neither PATH nor the build tree can satisfy
        # these checks. A plain renamed script is rejected by the header check.
        observed_version = command([str(executable), "--version"], "archive-extracted --version")
        require(observed_version == f"shipper {version}",
                f"packaged version mismatch: approved shipper {version}, observed {observed_version!r}")
        help_output = command([str(executable), "--help"], "archive-extracted --help")
        require(all(word in help_output for word in ["Usage:", "shipper", "plan", "publish", "resume"]),
                "archive-extracted help is missing the supported CLI entry points")
        plan_check = offline_plan(executable, Path(scratch))
    rustc = command(["rustc", "-vV"], "actual Rust toolchain read")
    cargo = command(["cargo", "--version"], "actual Cargo toolchain read")
    require(f"host: {target}" in rustc.splitlines(), f"Rust toolchain host does not match native target {target}")
    require(source_identity(sha, tree) == lock_digest, "approved Cargo.lock changed during smoke")
    require(sha256_file(archive) == archive_digest, "archive changed during smoke")
    receipt = {
        "schema_version": SCHEMA, "result": "success", "target": target,
        "source_sha": sha, "source_tree": tree, "approved_version": version,
        "lockfile_sha256": lock_digest, "artifact": archive.name,
        "archive_sha256": archive_digest, "executable_sha256": digest(data),
        "host": host, "toolchain": {"rustc_verbose": rustc, "cargo": cargo},
        "checks": {
            "version": {"result": "success", "exit_code": 0, "stdout": observed_version},
            "help": {"result": "success", "exit_code": 0, "stdout_sha256": digest(help_output.encode())},
            "plan": plan_check,
        },
    }
    write_json(output, receipt)
    return receipt


def aggregate(artifacts, sha, tree, version, output):
    validate_approval(sha, tree, version)
    lock_digest = source_identity(sha, tree)
    accepted = []
    for target in TARGETS:
        archive = artifacts / f"shipper-{target}" / canonical_archive(target)
        receipt_path = archive.with_name(archive.name + ".smoke.json")
        require(archive.is_file(), f"missing archive for target {target}")
        require(receipt_path.is_file(), f"missing native executable smoke receipt for target {target}")
        receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
        require(isinstance(receipt, dict), f"invalid smoke receipt for target {target}")
        expected = {
            "schema_version": SCHEMA, "result": "success", "target": target,
            "source_sha": sha, "source_tree": tree, "approved_version": version,
            "lockfile_sha256": lock_digest, "artifact": archive.name,
            "archive_sha256": sha256_file(archive),
        }
        for field, value in expected.items():
            require(receipt.get(field) == value, f"{target}: smoke receipt {field} mismatch or missing proof")
        data, _ = archive_payload(archive, target)
        require(receipt.get("executable_sha256") == digest(data), f"{target}: extracted executable digest mismatch")
        require(receipt.get("host") == {"system": TARGETS[target][1], "machine": TARGETS[target][2]},
                f"{target}: native host evidence mismatch")
        toolchain = receipt.get("toolchain", {})
        require(isinstance(toolchain, dict) and isinstance(toolchain.get("rustc_verbose"), str)
                and f"host: {target}" in toolchain["rustc_verbose"].splitlines()
                and isinstance(toolchain.get("cargo"), str) and toolchain["cargo"].startswith("cargo "),
                f"{target}: actual toolchain evidence missing")
        checks = receipt.get("checks", {})
        require(isinstance(checks, dict)
                and checks.get("version") == {"result": "success", "exit_code": 0, "stdout": f"shipper {version}"},
                f"{target}: version proof missing, skipped, or wrong")
        help_check = checks.get("help", {})
        require(isinstance(help_check, dict) and help_check.get("result") == "success" and help_check.get("exit_code") == 0
                and re.fullmatch(r"[0-9a-f]{64}", str(help_check.get("stdout_sha256"))) is not None,
                f"{target}: help proof missing, skipped, or invalid")
        plan_check = checks.get("plan", {})
        require(isinstance(plan_check, dict), f"{target}: offline plan proof missing or invalid")
        for field, value in {"result": "success", "exit_code": 0, "cargo_offline": True,
                             "schema_version": "shipper.plan.v1", "publication_performed": False,
                             "package": SMOKE_PACKAGE, "registry": SMOKE_REGISTRY}.items():
            require(plan_check.get(field) == value, f"{target}: offline plan {field} mismatch or missing proof")
        for field in ("plan_id", "stdout_sha256"):
            require(re.fullmatch(r"[0-9a-f]{64}", str(plan_check.get(field))) is not None,
                    f"{target}: offline plan {field} missing or invalid")
        accepted.append({"target": target, "archive_sha256": expected["archive_sha256"], "smoke_receipt": str(receipt_path)})
    summary = {"schema_version": "shipper.release_binary_smoke_set.v1", "result": "success",
               "source_sha": sha, "source_tree": tree, "approved_version": version, "targets": accepted}
    write_json(output, summary)
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["smoke", "aggregate"])
    parser.add_argument("--approved-sha", required=True)
    parser.add_argument("--approved-tree", required=True)
    parser.add_argument("--approved-version", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target", choices=TARGETS)
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--artifacts", type=Path)
    args = parser.parse_args()
    common = (args.approved_sha, args.approved_tree, args.approved_version, args.output)
    if args.mode == "smoke":
        require(args.archive is not None and args.target is not None, "smoke requires --archive and --target")
        smoke(args.archive, args.target, *common)
        print(f"PASS: extracted {args.target} executable matches approved version, help, and offline plan")
    else:
        require(args.artifacts is not None, "aggregate requires --artifacts")
        aggregate(args.artifacts, *common)
        print("PASS: all four archive digests have successful matching native executable receipts")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, json.JSONDecodeError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        sys.exit(1)
