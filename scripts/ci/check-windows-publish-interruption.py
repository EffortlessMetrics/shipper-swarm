"""Watchdog-controlled Windows interruption after Cargo exit, using loopback and fake Cargo."""
import argparse
import ctypes
from ctypes import wintypes
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time

NEW_GROUP = 0x200
NEW_CONSOLE = 0x10
CTRL_BREAK = 1
HERE = Path(__file__).resolve()
KERNEL = ctypes.WinDLL("kernel32", use_last_error=True)
KERNEL.GenerateConsoleCtrlEvent.argtypes = (wintypes.DWORD, wintypes.DWORD)
KERNEL.GenerateConsoleCtrlEvent.restype = wintypes.BOOL
KERNEL.GetConsoleProcessList.argtypes = (ctypes.POINTER(wintypes.DWORD), wintypes.DWORD)
KERNEL.GetConsoleProcessList.restype = wintypes.DWORD


def cargo_fixture(root):
    with (root / "publish-calls").open("a") as f:
        f.write("publish\n")
    (root / "unknown-registry").touch()
    print("error: connection reset by peer", file=sys.stderr, flush=True)
    if os.environ["FIXTURE_STAGE"] == "timeout":
        time.sleep(12)
    raise SystemExit(42)


def cli_fixture(sh, root, stage, parallel):
    entered = threading.Event()
    release = threading.Event()

    class Registry(BaseHTTPRequestHandler):
        def do_GET(self):
            assert self.path == "/api/v1/crates/demo/0.1.0", self.path
            status = 503 if (root / "unknown-registry").exists() else 404
            if status == 503:
                entered.set()
                assert release.wait(12), "registry handler watchdog expired"
            payload = b"{}"
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            try:
                self.end_headers()
                self.wfile.write(payload)
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
                pass

        def log_message(self, *args):
            pass

    (root / "demo/src").mkdir(parents=True)
    (root / "Cargo.toml").write_text('[workspace]\nmembers=["demo"]\nresolver="2"\n')
    (root / "demo/Cargo.toml").write_text(
        '[package]\nname="demo"\nversion="0.1.0"\nedition="2021"\n')
    (root / "demo/src/lib.rs").write_text("pub fn demo() {}\n")
    proxy = root / "fake-cargo.cmd"
    proxy.write_text(
        '@echo off\nif /i "%~1"=="publish" goto :publish\n'
        'if /i "%~1"=="metadata" goto :cargo\n'
        'if /i "%~1"=="--version" goto :cargo\n'
        'exit /b 88\n:publish\n'
        'start "" /b /wait "%FIXTURE_PYTHON%" "%FIXTURE_SCRIPT%" fake "%FIXTURE_ROOT%"\n'
        'exit /b %errorlevel%\n:cargo\ncargo %*\nexit /b %errorlevel%\n')
    env = dict(os.environ, CARGO_NET_OFFLINE="true", RUSTUP_TOOLCHAIN="1.95.0",
               SHIPPER_CARGO_BIN=str(proxy), FIXTURE_ROOT=str(root),
               FIXTURE_PYTHON=sys.executable, FIXTURE_SCRIPT=str(HERE), FIXTURE_STAGE=stage)
    server = ThreadingHTTPServer(("127.0.0.1", 0), Registry)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    args = [str(sh), "--manifest-path", str(root / "Cargo.toml"),
            "--state-dir", str(root / "state"),
            "--api-base", f"http://127.0.0.1:{server.server_port}",
            "--allow-loopback", "--allow-dirty", "--skip-ownership-check",
            "--no-verify", "--max-attempts", "3", "--output-lines", "5",
            "--per-package-timeout", "2s", "--format", "json",
            "--readiness-method", "api", "--readiness-poll", "0ms",
            "--readiness-timeout", "1s"]
    if parallel:
        args += ["--parallel", "--max-concurrent", "2"]
    event_path = root / "state/events.jsonl"
    try:
        with (root / "cli.stdout").open("wb") as out, (root / "cli.stderr").open("wb") as err:
            proc = subprocess.Popen(args + ["publish"], cwd=root, env=env, stdin=subprocess.DEVNULL,
                                    stdout=out, stderr=err, creationflags=NEW_GROUP)
            try:
                assert entered.wait(10), "CLI never reached the post-Cargo registry check"
                members = (wintypes.DWORD * 64)()
                count = KERNEL.GetConsoleProcessList(members, len(members))
                assert 1 < count <= len(members)
                assert os.getpid() in members[:count] and proc.pid in members[:count]
                assert proc.poll() is None
                assert KERNEL.GenerateConsoleCtrlEvent(CTRL_BREAK, proc.pid), ctypes.get_last_error()
                # The registry barrier stays closed until after CLI exit.
                assert proc.wait(timeout=4) & 0xffffffff == 0xc000013a
            finally:
                release.set()
                if proc.poll() is None:
                    proc.kill()
                    proc.wait(timeout=2)

        initial = event_path.read_bytes()
        events = [json.loads(line)["event_type"] for line in initial.splitlines()]
        assert any(event["type"] == "package_output" for event in events), events
        assert all(event["type"] not in ("package_published", "publish_reconciled")
                   for event in events), events
        state = json.loads((root / "state/state.json").read_bytes())
        checkpoint = [event for event in events if event["type"] == "package_upload_unconfirmed"]

        resumed = subprocess.run(args + ["resume"], env=env, cwd=root,
                                 stdin=subprocess.DEVNULL, capture_output=True, timeout=12)
        following = event_path.read_bytes()
        uploads = (root / "publish-calls").read_text().splitlines()
        live_state = state["packages"]["demo@0.1.0"]["state"]["state"]
        print(json.dumps(dict(stage=stage, parallel=parallel, exit=proc.returncode,
                              live_state=live_state, checkpoint=bool(checkpoint),
                              upload_attempts=len(uploads), resume=resumed.returncode)), flush=True)
        assert live_state == "ambiguous"
        assert [event["class"] for event in checkpoint] == [
            "ambiguous" if stage == "timeout" else "retryable"
        ], checkpoint
        assert resumed.returncode != 0 and len(uploads) == 1, resumed.stderr
        assert following.startswith(initial)
        assert b"still_unknown" in following[len(initial):], following
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)


def main():
    if sys.argv[1] == "fake":
        cargo_fixture(Path(sys.argv[2]))
    elif sys.argv[1] == "child":
        cli_fixture(Path(sys.argv[2]), Path(sys.argv[3]), sys.argv[4], sys.argv[5] == "1")
    else:
        parser = argparse.ArgumentParser()
        parser.add_argument("stage", choices=["nonzero", "timeout"])
        parser.add_argument("binary", type=Path)
        options = parser.parse_args()
        startup = subprocess.STARTUPINFO()
        startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startup.wShowWindow = subprocess.SW_HIDE
        for parallel in [0, 1]:
            with tempfile.TemporaryDirectory(prefix="shipper 460-") as root:
                result = subprocess.run(
                    [sys.executable, str(HERE), "child", str(options.binary.resolve()),
                     root, options.stage, str(parallel)],
                    stdin=subprocess.DEVNULL, capture_output=True, timeout=35,
                    creationflags=NEW_CONSOLE, startupinfo=startup)
                print(result.stdout.decode("utf-8", errors="replace").rstrip(), flush=True)
                if result.returncode:
                    print(result.stderr.decode("utf-8", errors="replace").rstrip(), flush=True)
                    for name in ["cli.stdout", "cli.stderr"]:
                        value = Path(root, name)
                        if value.exists():
                            print(name, value.read_text(errors="replace"), flush=True)
                    raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
