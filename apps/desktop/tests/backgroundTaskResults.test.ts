import assert from "node:assert/strict";
import test from "node:test";
import type { OperationState } from "../src/api/tasks.ts";
import { operationResult } from "../src/app/backgroundTaskResult.ts";

function operation(overrides: Partial<OperationState> = {}): OperationState {
  return {
    module: "taxonomy",
    task_id: "task-1",
    task_kind: null,
    task_scope: null,
    state: "completed",
    operation: "execute_custom_taxonomy_sql",
    started_at: "2026-08-21 10:00:00.000000",
    finished_at: "2026-08-21 10:00:01.000000",
    progress: null,
    result: { changeset_size: 3 },
    error: null,
    ...overrides,
  };
}

test("reads the result only from the exact completed task", () => {
  const completed = operation({ task_id: "custom-sql-1" });

  assert.deepEqual(operationResult<{ changeset_size: number }>(completed, "custom-sql-1"), {
    changeset_size: 3,
  });
  assert.throws(
    () => operationResult(completed, "custom-sql-2"),
    /returned a different task result/,
  );
});

test("surfaces failed background task errors", () => {
  const failed = operation({
    task_id: "formatted-preview-1",
    operation: "preview_taxonomy_rows",
    state: "failed",
    result: null,
    error: "operation cancelled",
  });

  assert.throws(
    () => operationResult(failed, "formatted-preview-1"),
    /operation cancelled/,
  );
});
