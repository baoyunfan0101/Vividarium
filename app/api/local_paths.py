"""Local file and directory picker endpoints for the desktop UI."""

from __future__ import annotations

import shutil
import subprocess
import sys

from fastapi import APIRouter, HTTPException


router = APIRouter(prefix="/local", tags=["local"])


@router.get("/select-directory")
def select_directory() -> dict:
    return {"path": _ask_directory()}


@router.get("/select-file")
def select_file() -> dict:
    return {"path": _ask_file()}


def _ask_directory() -> str | None:
    if sys.platform == "darwin":
        return _run_osascript(
            'POSIX path of (choose folder with prompt "Select photo root")'
        )
    if sys.platform.startswith("win"):
        return _run_powershell(
            """
            Add-Type -AssemblyName System.Windows.Forms
            $dialog = New-Object System.Windows.Forms.FolderBrowserDialog
            if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
              Write-Output $dialog.SelectedPath
            }
            """
        )
    if shutil.which("zenity"):
        return _run_picker_command(["zenity", "--file-selection", "--directory"])
    return _ask_directory_with_tkinter()


def _ask_file() -> str | None:
    if sys.platform == "darwin":
        return _run_osascript(
            'POSIX path of (choose file with prompt "Select knowledge base")'
        )
    if sys.platform.startswith("win"):
        return _run_powershell(
            """
            Add-Type -AssemblyName System.Windows.Forms
            $dialog = New-Object System.Windows.Forms.OpenFileDialog
            $dialog.Filter = "Excel workbooks (*.xlsx;*.xlsm;*.xls)|*.xlsx;*.xlsm;*.xls|All files (*.*)|*.*"
            if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
              Write-Output $dialog.FileName
            }
            """
        )
    if shutil.which("zenity"):
        return _run_picker_command(
            [
                "zenity",
                "--file-selection",
                "--file-filter=Excel workbooks | *.xlsx *.xlsm *.xls",
                "--file-filter=All files | *",
            ]
        )
    return _ask_file_with_tkinter()


def _run_osascript(script: str) -> str | None:
    return _run_picker_command(["osascript", "-e", script])


def _run_powershell(script: str) -> str | None:
    executable = shutil.which("powershell") or shutil.which("pwsh")
    if not executable:
        raise HTTPException(
            status_code=503,
            detail="PowerShell is required for the local path picker on Windows.",
        )
    return _run_picker_command(
        [executable, "-NoProfile", "-STA", "-Command", script]
    )


def _run_picker_command(command: list[str]) -> str | None:
    try:
        completed = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError as exc:
        raise HTTPException(
            status_code=503,
            detail="Local path picker cannot be opened from this environment.",
        ) from exc

    if completed.returncode == 0:
        path = completed.stdout.strip()
        return path or None

    message = completed.stderr.strip()
    if "User canceled" in message or "cancel" in message.lower():
        return None
    raise HTTPException(
        status_code=503,
        detail=message or "Local path picker cannot be opened from this environment.",
    )


def _ask_directory_with_tkinter() -> str | None:
    try:
        from tkinter import TclError, Tk, filedialog
    except ImportError as exc:
        raise HTTPException(
            status_code=503,
            detail="Local directory picker is not available in this Python environment.",
        ) from exc

    try:
        root = _hidden_root(Tk)
        try:
            path = filedialog.askdirectory(parent=root)
            return path or None
        finally:
            root.destroy()
    except TclError as exc:
        raise HTTPException(
            status_code=503,
            detail="Local directory picker cannot be opened from this environment.",
        ) from exc


def _ask_file_with_tkinter() -> str | None:
    try:
        from tkinter import TclError, Tk, filedialog
    except ImportError as exc:
        raise HTTPException(
            status_code=503,
            detail="Local file picker is not available in this Python environment.",
        ) from exc

    try:
        root = _hidden_root(Tk)
        try:
            path = filedialog.askopenfilename(
                parent=root,
                filetypes=[
                    ("Excel workbooks", "*.xlsx *.xlsm *.xls"),
                    ("All files", "*.*"),
                ],
            )
            return path or None
        finally:
            root.destroy()
    except TclError as exc:
        raise HTTPException(
            status_code=503,
            detail="Local file picker cannot be opened from this environment.",
        ) from exc


def _hidden_root(tk_factory: type) -> object:
    root = tk_factory()
    root.withdraw()
    root.attributes("-topmost", True)
    root.update()
    return root
