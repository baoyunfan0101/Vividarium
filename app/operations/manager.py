"""In-process operation coordination for long local mutations."""

from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from copy import deepcopy
from datetime import datetime
from threading import Lock
from typing import Callable
from uuid import uuid4


MODULES = ("photos", "taxa", "mapping")


class OperationBusyError(RuntimeError):
    def __init__(self, module: str, blocked_by: str) -> None:
        super().__init__(f"{module} is blocked by {blocked_by}")
        self.module = module
        self.blocked_by = blocked_by


class OperationManager:
    def __init__(self) -> None:
        self._lock = Lock()
        self._executor = ThreadPoolExecutor(max_workers=3, thread_name_prefix="operation")
        self._states = {module: _idle_state(module) for module in MODULES}

    def start(
        self,
        module: str,
        operation: str,
        callback: Callable[[], object],
    ) -> dict:
        if module not in MODULES:
            raise ValueError(f"unknown operation module: {module}")

        with self._lock:
            blocked_by = self._blocked_by(module)
            if blocked_by:
                raise OperationBusyError(module, blocked_by)

            task_id = uuid4().hex
            self._states[module] = {
                "module": module,
                "task_id": task_id,
                "operation": operation,
                "running": True,
                "started_at": _now(),
                "finished_at": None,
                "message": f"{operation} running",
                "processed": 0,
                "total": None,
                "result": None,
                "error": None,
            }

        self._executor.submit(self._run, module, task_id, callback)
        return self.status(module)

    def status(self, module: str | None = None) -> dict:
        with self._lock:
            if module:
                return deepcopy(self._states[module])
            return {name: deepcopy(state) for name, state in self._states.items()}

    def progress(
        self,
        module: str,
        processed: int,
        total: int | None = None,
        message: str | None = None,
    ) -> None:
        with self._lock:
            state = self._states[module]
            if not state["running"]:
                return
            state["processed"] = processed
            state["total"] = total
            if message is not None:
                state["message"] = message

    def _run(
        self,
        module: str,
        task_id: str,
        callback: Callable[[], object],
    ) -> None:
        try:
            result = callback()
        except Exception as exc:
            with self._lock:
                if self._states[module]["task_id"] != task_id:
                    return
                self._states[module].update(
                    {
                        "running": False,
                        "finished_at": _now(),
                        "message": "failed",
                        "error": str(exc),
                    }
                )
            return

        with self._lock:
            if self._states[module]["task_id"] != task_id:
                return
            self._states[module].update(
                {
                    "running": False,
                    "finished_at": _now(),
                    "message": "completed",
                    "result": result,
                }
            )

    def _blocked_by(self, module: str) -> str | None:
        for other_module, state in self._states.items():
            if state["running"] and _conflicts(module, other_module):
                return other_module
        return None


def _idle_state(module: str) -> dict:
    return {
        "module": module,
        "task_id": None,
        "operation": None,
        "running": False,
        "started_at": None,
        "finished_at": None,
        "message": "idle",
        "processed": 0,
        "total": None,
        "result": None,
        "error": None,
    }


def _conflicts(module: str, running_module: str) -> bool:
    if module == running_module:
        return True
    if module == "mapping" or running_module == "mapping":
        return True
    return False


def _now() -> str:
    return datetime.now().isoformat(sep=" ", timespec="microseconds")


operation_manager = OperationManager()
