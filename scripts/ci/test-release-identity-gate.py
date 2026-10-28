"""Run the tag-only release gate with its production xtask validator, offline.

The workflow run block and step environment are consumed directly. Git is real;
the GitHub API and cargo metadata have bounded fixtures. The cargo shim only
dispatches the actual release-identity binary or serves fixture metadata, and
rejects every other command. This cannot publish or dispatch a workflow.
"""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile


PACKAGES = [
    "shipper", "shipper-cargo-failure", "shipper-cli", "shipper-config",
    "shipper-core", "shipper-duration", "shipper-encrypt",
    "shipper-output-sanitizer", "shipper-registry", "shipper-retry",
    "shipper-sparse-index", "shipper-types", "shipper-webhook",
]
TOKEN = "fixture-read-token-not-a-credential"


def gate_step(workflow):
    job = workflow.split("  release-identity-gate:\n", 1)[1].split("\n  build-binaries:", 1)[0]
    step = job.split("      - name: Validate approved source identity\n", 1)[1].split("\n      - ", 1)[0]
    before_run, body = step.split("        run: |\n", 1)
    script = "\n".join(line[10:] if line.startswith("          ") else line for line in body.splitlines())
    environment = {}
    if "        env:\n" in before_run:
        for line in before_run.split("        env:\n", 1)[1].splitlines():
            match = re.fullmatch(r"          ([A-Z_]+): (.+)", line)
            if match:
                key, value = match.groups()
                if value == "${{ secrets.GITHUB_TOKEN }}":
                    environment[key] = TOKEN
                else:
                    raise AssertionError(f"unrecognized gate environment input: {key}")
    return script, environment


def write_executable(path, text):
    path.write_text(text, encoding="utf-8")
    path.chmod(0o755)


def checked(command, cwd, env):
    return subprocess.check_output(command, cwd=cwd, env=env, text=True).strip()


def run_case(workflow, xtask, *, event="push", response="valid", strip_binding=False):
    script, step_environment = gate_step(workflow)
    if strip_binding:
        step_environment.pop("GH_TOKEN", None)
    with tempfile.TemporaryDirectory(prefix="shipper-release-gate-") as temp:
        root = Path(temp)
        shims = root / "bin"
        shims.mkdir()
        home = root / "home"
        home.mkdir()
        gh_config = root / "gh-empty"
        gh_config.mkdir()
        # Do not inherit Git overrides or ambient CLI credentials.
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(("GIT_", "GH_", "GITHUB_"))}
        env.update({
            "HOME": str(home), "GH_CONFIG_DIR": str(gh_config),
            "GIT_AUTHOR_NAME": "Release fixture", "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
            "GIT_COMMITTER_NAME": "Release fixture", "GIT_COMMITTER_EMAIL": "fixture@example.invalid",
            "GIT_AUTHOR_DATE": "2026-10-28T12:00:00Z", "GIT_COMMITTER_DATE": "2026-10-28T12:00:00Z",
        })
        git = shutil.which("git")
        if not git:
            raise AssertionError("git is required for the source identity fixture")
        checked([git, "init", "-q"], root, env)
        (root / "CHANGELOG.md").write_text("## [0.5.0] - 2026-10-28\n", encoding="utf-8")
        (root / "RELEASE_NOTES_v0.5.0.md").write_text("Fixture release notes\n", encoding="utf-8")
        checked([git, "add", "CHANGELOG.md", "RELEASE_NOTES_v0.5.0.md"], root, env)
        checked([git, "commit", "-qm", "fixture source"], root, env)
        checked([git, "tag", "-a", "v0.5.0", "-m", "fixture tag"], root, env)
        sha = checked([git, "rev-parse", "HEAD"], root, env)
        tree = checked([git, "rev-parse", "HEAD^{tree}"], root, env)
        metadata = {"packages": [{"name": name, "version": "0.5.0", "dependencies": [], "publish": None} for name in PACKAGES]}
        (root / "metadata.json").write_text(json.dumps(metadata), encoding="utf-8")
        write_executable(shims / "gh", "#!" + sys.executable + "\n" + '''
import json, os, pathlib, sys
root = pathlib.Path(os.environ["FIXTURE_ROOT"])
with (root / "gh-calls.jsonl").open("a") as stream:
    stream.write(json.dumps(sys.argv[1:]) + "\\n")
assert sys.argv[1:] == ["api", "repos/EffortlessMetrics/shipper/git/ref/heads/main", "--jq", ".object.sha"]
if os.environ.get("GH_TOKEN") != "fixture-read-token-not-a-credential":
    print("fixture gh: missing explicit read token", file=sys.stderr)
    sys.exit(4)
response = os.environ["FIXTURE_RESPONSE"]
if response == "auth_failure":
    print("fixture gh: HTTP 401", file=sys.stderr)
    sys.exit(1)
if response == "api_missing":
    print("fixture gh: HTTP 404", file=sys.stderr)
    sys.exit(1)
print({"valid": os.environ["APPROVED_SHA"], "empty": "", "null": "null", "malformed": "not-a-sha", "drift": "d" * 40}[response])
''')
        write_executable(shims / "cargo", "#!" + sys.executable + "\n" + '''
import json, os, pathlib, sys
root = pathlib.Path(os.environ["FIXTURE_ROOT"])
with (root / "cargo-calls.jsonl").open("a") as stream:
    stream.write(json.dumps(sys.argv[1:]) + "\\n")
if sys.argv[1:] == ["metadata", "--format-version", "1", "--no-deps"]:
    print((root / "metadata.json").read_text())
elif sys.argv[1:3] == ["xtask", "release-identity"]:
    os.execv(os.environ["FIXTURE_XTASK"], [os.environ["FIXTURE_XTASK"]] + sys.argv[2:])
else:
    raise SystemExit("unexpected cargo command; publication is prohibited")
''')
        env.update({
            "PATH": str(shims) + os.pathsep + os.environ["PATH"],
            "FIXTURE_ROOT": str(root), "FIXTURE_XTASK": str(xtask),
            "FIXTURE_RESPONSE": response, "GITHUB_EVENT_NAME": event,
            "GITHUB_REPOSITORY": "EffortlessMetrics/shipper", "GITHUB_REF_NAME": "v0.5.0",
            "GITHUB_ACTIONS": "true", "GITHUB_OUTPUT": str(root / "outputs"),
            "APPROVED_SHA": sha, "APPROVED_TREE": tree, "APPROVED_VERSION": "0.5.0",
            "APPROVAL_RECORD_REF": "fixture#477", "APPROVAL_RECORD_SHA": "fixture-record",
            "APPROVED_REGISTRY": "crates-io", "APPROVED_AUTH_POSTURE": "trusted_publishing",
        })
        env.update(step_environment)
        result = subprocess.run(["bash", "-c", script], cwd=root, env=env, text=True, capture_output=True, timeout=10)
        identity_path = root / "target/policy/release-identity.json"
        identity = json.loads(identity_path.read_text()) if identity_path.exists() else None
        outputs = (root / "outputs").read_text() if (root / "outputs").exists() else ""
        calls = {}
        for name in ["gh", "cargo"]:
            path = root / (name + "-calls.jsonl")
            calls[name] = [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
        return result, identity, outputs, calls, sha


def test(workflow, xtask):
    result, identity, outputs, calls, sha = run_case(workflow, xtask)
    assert result.returncode == 0, result.stderr
    assert identity["approved_sha"] == sha and identity["tag"] == "v0.5.0"
    assert f"approved_sha={sha}\n" in outputs
    assert len(calls["gh"]) == 1
    assert "--main-sha" in calls["cargo"][0] and "--tag-date" in calls["cargo"][0]
    for response, needle in [
        ("auth_failure", "HTTP 401"), ("api_missing", "HTTP 404"),
        ("empty", "40-character hexadecimal SHA"), ("null", "40-character hexadecimal SHA"),
        ("malformed", "40-character hexadecimal SHA"), ("drift", "release-authority main mismatch"),
    ]:
        result, identity, outputs, calls, _ = run_case(workflow, xtask, response=response)
        assert result.returncode != 0 and needle in result.stderr, (response, result.stderr)
        assert identity is None and not outputs, response
        assert len(calls["gh"]) == 1, response
    result, identity, outputs, calls, _ = run_case(workflow, xtask, strip_binding=True)
    assert result.returncode != 0 and "missing explicit read token" in result.stderr
    assert identity is None and not outputs and not calls["cargo"]
    result, identity, outputs, calls, _ = run_case(workflow, xtask, event="workflow_dispatch", strip_binding=True)
    assert result.returncode == 0 and identity["tag"] is None, result.stderr
    assert not calls["gh"] and "--main-sha" not in calls["cargo"][0]
    print("Passed 9 actual-step cases: authenticated tag, six failed reads, removed binding, non-tag dispatch.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--xtask", type=Path, required=True)
    parser.add_argument("--workflow", type=Path, default=Path(".github/workflows/release.yml"))
    args = parser.parse_args()
    test(args.workflow.read_text(encoding="utf-8"), args.xtask.resolve(strict=True))


if __name__ == "__main__":
    main()
