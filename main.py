"""Local development launcher for PhytoIndex."""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import threading
import time
import webbrowser
from pathlib import Path

import uvicorn


ROOT = Path(__file__).resolve().parent
FRONTEND = ROOT / "frontend"


def main() -> None:
    args = parse_args()
    if args.production or is_frozen():
        start_production(args)
        return

    processes: list[subprocess.Popen[bytes]] = []

    try:
        if not args.frontend_only:
            processes.append(start_backend(args))
        if not args.backend_only:
            processes.append(start_frontend(args))

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
    parser.add_argument(
        "--production",
        action="store_true",
        help="serve the built frontend from FastAPI instead of starting Vite",
    )
    parser.add_argument("--host", default="127.0.0.1", help="backend host")
    parser.add_argument("--port", type=int, default=8000, help="backend port")
    parser.add_argument("--no-browser", action="store_true", help="do not open a browser")
    parser.add_argument(
        "--backend-path",
        action="append",
        default=[],
        help=(
            "prepend one or more paths to PATH for the backend process. "
            "Can also be set with PHYTOINDEX_BACKEND_PATH."
        ),
    )
    parser.add_argument(
        "--frontend-path",
        action="append",
        default=[],
        help=(
            "prepend one or more paths to PATH for the frontend process. "
            "Can also be set with PHYTOINDEX_FRONTEND_PATH."
        ),
    )
    return parser.parse_args()


def start_backend(args: argparse.Namespace) -> subprocess.Popen[bytes]:
    return subprocess.Popen(
        [
            sys.executable,
            "-m",
            "uvicorn",
            "app.api.main:app",
            "--reload",
            "--host",
            args.host,
            "--port",
            str(args.port),
        ],
        cwd=ROOT,
        env=process_env("PHYTOINDEX_BACKEND_PATH", args.backend_path),
    )


def start_frontend(args: argparse.Namespace) -> subprocess.Popen[bytes]:
    npm = "npm.cmd" if os.name == "nt" else "npm"
    return subprocess.Popen(
        [npm, "run", "dev"],
        cwd=FRONTEND,
        env=process_env("PHYTOINDEX_FRONTEND_PATH", args.frontend_path),
    )


def start_production(args: argparse.Namespace) -> None:
    os.environ.setdefault("PHYTOINDEX_SERVE_FRONTEND", "1")
    url = f"http://{args.host}:{args.port}"
    print("Starting PhytoIndex.")
    print(f"Open: {url}")
    print("Press Ctrl+C to stop.")
    if not args.no_browser:
        threading.Timer(1.0, lambda: webbrowser.open(url)).start()
    uvicorn.run(
        "app.api.main:app",
        host=args.host,
        port=args.port,
        reload=False,
        log_level="info",
    )


def is_frozen() -> bool:
    return bool(getattr(sys, "frozen", False))


def process_env(env_name: str, extra_paths: list[str]) -> dict[str, str]:
    env = os.environ.copy()
    configured = split_path_entries(env.get(env_name, ""))
    paths = [*extra_paths, *configured]
    if paths:
        env["PATH"] = os.pathsep.join(
            [normalize_path(path) for path in paths] + [env.get("PATH", "")]
        )
    return env


def split_path_entries(value: str) -> list[str]:
    if not value:
        return []
    return [item for item in value.split(os.pathsep) if item]


def normalize_path(value: str) -> str:
    return str(Path(value).expanduser().resolve())


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
