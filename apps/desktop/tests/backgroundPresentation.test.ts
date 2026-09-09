import assert from "node:assert/strict";
import test from "node:test";
import type { OperationState, OperationsStatus } from "../src/api/tasks.ts";
import {
  backgroundActiveCount,
  backgroundElapsed,
  backgroundProgressMode,
  backgroundProgressText,
  backgroundStageLabel,
  backgroundTaskSections,
  backgroundTaskTitle,
  formatBackgroundBytes,
  formatElapsed,
} from "../src/app/backgroundPresentation.ts";

function operation(overrides: Partial<OperationState> = {}): OperationState {
  return {
    module: "sql_import",
    task_id: "task-1",
    task_kind: null,
    task_scope: null,
    state: "running",
    operation: "validate_sql_import",
    started_at: "2026-08-21 10:00:00.000000",
    finished_at: null,
    progress: {
      stage: "normalizing_names",
      current: 1_320_000,
      total: 1_411_000,
      unit: "names",
    },
    result: null,
    error: null,
    ...overrides,
  };
}

test("maps task titles and stage labels without exposing ordinary snake case", () => {
  assert.equal(backgroundTaskTitle(operation()), "SQL import validation");
  assert.equal(backgroundTaskTitle(operation({ operation: "rename_directory_from_taxonomy" })), "Rename photos recursively");
  assert.equal(backgroundTaskTitle(operation({ operation: "execute_custom_taxonomy_sql" })), "Custom SQL");
  assert.equal(backgroundTaskTitle(operation({ operation: "export_custom_taxonomy_query" })), "Custom SQL export");
  assert.equal(backgroundTaskTitle(operation({ operation: "inspect_direct_import_database" })), "Direct import inspection");
  assert.equal(backgroundTaskTitle(operation({ operation: "preview_taxonomy_rows" })), "Formatted Update preview");
  assert.equal(backgroundTaskTitle(operation({ operation: "apply_taxonomy_rows" })), "Formatted Update");
  assert.equal(backgroundStageLabel(operation()), "Normalizing names");
  assert.equal(backgroundStageLabel(operation({
    progress: { stage: "executing_custom_sql", current: null, total: null, unit: null },
  })), "Executing Custom SQL");
  assert.equal(backgroundStageLabel(operation({
    progress: { stage: "applying_formatted_update", current: null, total: null, unit: null },
  })), "Applying Formatted Update");
  assert.equal(
    backgroundStageLabel(operation({
      operation: "unknown_background_task",
      progress: { stage: "unknown_background_stage", current: null, total: null, unit: null },
    })),
    "Unknown background stage",
  );
  assert.equal(backgroundTaskTitle(operation({ operation: "unknown_background_task" })), "Unknown background task");
});

test("formats determinate units and byte progress", () => {
  assert.equal(backgroundProgressMode(operation()), "determinate");
  assert.equal(backgroundProgressText(operation()), "1,320,000 / 1,411,000 names");
  const bytes = operation({
    progress: {
      stage: "fingerprinting_staging",
      current: 820 * 1024 * 1024,
      total: 1.4 * 1024 * 1024 * 1024,
      unit: "bytes",
    },
  });
  assert.equal(backgroundProgressText(bytes), "820 MB / 1.4 GB");
  assert.equal(formatBackgroundBytes(17), "17 B");
});

test("presents queued running completed and failed states consistently", () => {
  const queued = operation({ state: "queued", started_at: null, progress: null });
  const indeterminate = operation({
    progress: { stage: "checking_staging_database", current: null, total: null, unit: null },
  });
  const completed = operation({ state: "completed", finished_at: "2026-08-21 10:00:18.000000" });
  const failed = operation({ state: "failed", finished_at: "2026-08-21 10:00:20.000000", error: "foreign key check failed" });

  assert.equal(backgroundStageLabel(queued), "Queued");
  assert.equal(backgroundProgressMode(queued), "none");
  assert.equal(backgroundProgressMode(indeterminate), "indeterminate");
  assert.equal(backgroundProgressText(indeterminate), null);
  assert.equal(backgroundStageLabel(completed), "Completed");
  assert.equal(backgroundProgressMode(completed), "none");
  assert.equal(backgroundStageLabel(failed), "Failed");
});

test("formats running elapsed time and finished duration", () => {
  const startedAt = Date.parse("2026-08-21T10:00:00.000");
  assert.equal(backgroundElapsed(operation(), startedAt + 37_000), "0:37");
  assert.equal(backgroundElapsed(operation({
    state: "completed",
    finished_at: "2026-08-21 10:00:18.000000",
  }), startedAt + 100_000), "0:18");
  assert.equal(formatElapsed(3_734_000), "1:02:14");
});

test("orders active tasks before the ten newest recent tasks", () => {
  const operations: OperationsStatus = {};
  operations.running = operation({ task_id: "running" });
  operations.queued = operation({ task_id: "queued", state: "queued", started_at: null, progress: null });
  for (let index = 0; index < 12; index += 1) {
    operations[`recent-${index}`] = operation({
      task_id: `recent-${index}`,
      state: index === 11 ? "failed" : "completed",
      finished_at: `2026-08-21 10:${String(index).padStart(2, "0")}:00.000000`,
      error: index === 11 ? "failed" : null,
    });
  }

  const sections = backgroundTaskSections(operations);
  assert.equal(backgroundActiveCount(operations), 2);
  assert.deepEqual(sections.active.map((item) => item.task_id), ["running", "queued"]);
  assert.equal(sections.recent.length, 10);
  assert.equal(sections.recent[0].task_id, "recent-11");
  assert.equal(sections.recent[9].task_id, "recent-2");
});
