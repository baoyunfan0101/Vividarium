"""Local development launcher for PhytoIndex."""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parent
FRONTEND = ROOT / "frontend"


def main() -> None:
    args = parse_args()
    processes: list[subprocess.Popen[bytes]] = []

    try:
        if not args.frontend_only:
            processes.append(start_backend())
        if not args.backend_only:
            processes.append(start_frontend())

        print("PhytoIndex is starting.")
        if not args.frontend_only:
            print("Backend:  http://127.0.0.1:8000")
        if not args.backend_only:
            print("Frontend: http://127.0.0.1:5173")
        print("Press Ctrl+C to stop.")

        wait_for_processes(processes)
    except KeyboardInterrupt:
        print("\nStopping PhytoIndex...")
    finally:
        stop_processes(processes)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Start the local PhytoIndex app.")
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--backend-only", action="store_true", help="start only FastAPI")
    group.add_argument("--frontend-only", action="store_true", help="start only Vite")
    return parser.parse_args()


def start_backend() -> subprocess.Popen[bytes]:
    return subprocess.Popen(
        [
            sys.executable,
            "-m",
            "uvicorn",
            "app.api.main:app",
            "--reload",
            "--host",
            "127.0.0.1",
            "--port",
            "8000",
        ],
        cwd=ROOT,
    )


def start_frontend() -> subprocess.Popen[bytes]:
    npm = "npm.cmd" if os.name == "nt" else "npm"
    return subprocess.Popen([npm, "run", "dev"], cwd=FRONTEND)


def wait_for_processes(processes: list[subprocess.Popen[bytes]]) -> None:
    while processes:
        for process in list(processes):
            code = process.poll()
            if code is None:
                continue
            processes.remove(process)
            if code != 0:
                raise SystemExit(code)
        time.sleep(0.5)


def stop_processes(processes: list[subprocess.Popen[bytes]]) -> None:
    for process in processes:
        if process.poll() is None:
            process.terminate()
    for process in processes:
        if process.poll() is None:
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()


if __name__ == "__main__":
    main()
