"""Exercise the actual release resume artifact selector without network access.

The gh stub serves paginated API fixtures; Bash and jq execute the workflow's
production selector. Action inputs are checked against download-artifact v8's
single-ID contract (contents extracted directly into path, no name wrapper).
"""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


def step(job, name):
    return job.split("      - name: " + name + "\n", 1)[1].split("\n      - ", 1)[0]


def selector(workflow):
    job = workflow.split("  release-resume:\n", 1)[1]
    selected = step(job, "Select prior .shipper/ state artifact")
    body = selected.split("        run: |\n", 1)[1]
    script = "\n".join(line[10:] if line.startswith("          ") else line for line in body.splitlines())
    assert "          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}" in selected
    assert "          ARTIFACT_RUN_ID: ${{ inputs.artifact_run_id }}" in selected
    download = step(job, "Download prior .shipper/ state")
    assert "uses: actions/download-artifact@v8" in download
    inputs = dict(re.findall(r"^          ([\w-]+): (.+)$", download, re.M))
    assert inputs == {
        "artifact-ids": "${{ steps.prior-state.outputs.artifact_id }}",
        "path": ".shipper/", "run-id": "${{ github.event.inputs.artifact_run_id }}",
        "github-token": "${{ secrets.GITHUB_TOKEN }}",
    }, inputs
    return script


def artifact(**changes):
    return {"id": 123, "name": "shipper-state-final", "expired": False,
            "workflow_run": {"id": 17}, **changes}


def run(script, pages, run_id="17", api_failure=False):
    with tempfile.TemporaryDirectory(prefix="shipper-artifact-selector-") as temp:
        root = Path(temp)
        (root / "response").write_text(pages if isinstance(pages, str) else json.dumps(pages))
        gh = root / "gh"
        gh.write_text("#!" + sys.executable + "\n" + '''
import os, pathlib, sys
root = pathlib.Path(os.environ["FIXTURE_ROOT"])
(root / "called").write_text("called")
assert sys.argv[1:] == ["api", "--paginate", "--slurp", "repos/EffortlessMetrics/shipper/actions/runs/17/artifacts?per_page=100"]
assert os.environ.get("GH_TOKEN") == "fixture-actions-read"
if os.environ["API_FAILURE"] == "true":
    print("fixture HTTP 403", file=sys.stderr)
    sys.exit(1)
print((root / "response").read_text())
''')
        gh.chmod(0o755)
        env = {key: value for key, value in os.environ.items() if not key.startswith(("GH_", "GITHUB_"))}
        env.update({"PATH": str(root) + os.pathsep + env["PATH"], "FIXTURE_ROOT": str(root),
                    "GH_TOKEN": "fixture-actions-read", "ARTIFACT_RUN_ID": run_id,
                    "GITHUB_REPOSITORY": "EffortlessMetrics/shipper",
                    "GITHUB_OUTPUT": str(root / "outputs"), "GITHUB_STEP_SUMMARY": str(root / "summary"),
                    "API_FAILURE": str(api_failure).lower()})
        result = subprocess.run(["bash", "-c", script], cwd=root, env=env, capture_output=True, text=True, timeout=5)
        output = (root / "outputs").read_text() if (root / "outputs").exists() else ""
        summary = (root / "summary").read_text() if (root / "summary").exists() else ""
        return result, output, summary, (root / "called").exists()


def test(workflow):
    script = selector(workflow)
    # Include an unrelated first page: --paginate --slurp must feed every page.
    valid = [{"artifacts": [artifact(id=99, name="unrelated")]}, {"artifacts": [artifact()]}]
    result, output, summary, called = run(script, valid)
    assert result.returncode == 0 and output == "artifact_id=123\n", result.stderr
    assert called and "run `17`, artifact `123`" in summary
    failures = [
        ([{"artifacts": []}], "missing or ambiguous evidence"),
        ([{"artifacts": [artifact()]}, {"artifacts": [artifact(id=124)]}], "missing or ambiguous evidence"),
        ([{"artifacts": [artifact(expired=True)]}], "expired or has unknown expiry"),
        ([{"artifacts": [artifact(expired=None)]}], "expired or has unknown expiry"),
        ([{"artifacts": [artifact(workflow_run={"id": 18})]}], "does not belong to the requested run"),
        ([{"artifacts": [artifact(id="123")]}], "invalid ID"),
        ([{"artifacts": [artifact(id=1.5)]}], "invalid ID"),
        ("not-json", "parse error"),
    ]
    for pages, expected in failures:
        result, output, summary, _ = run(script, pages)
        assert result.returncode != 0 and expected in result.stderr, result.stderr
        assert not output and not summary
    for invalid in ["", "-1", "17; echo bad", "017"]:
        result, output, summary, called = run(script, valid, run_id=invalid)
        assert result.returncode != 0 and "positive artifact_run_id" in result.stdout
        assert not output and not summary and not called
    result, output, summary, _ = run(script, valid, api_failure=True)
    assert result.returncode != 0 and "HTTP 403" in result.stderr
    assert not output and not summary
    print("Passed 14 actual-selector cases: paginated valid predecessor and 13 refusal controls.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workflow", type=Path, default=Path(".github/workflows/release.yml"))
    args = parser.parse_args()
    test(args.workflow.read_text(encoding="utf-8"))


if __name__ == "__main__":
    main()
