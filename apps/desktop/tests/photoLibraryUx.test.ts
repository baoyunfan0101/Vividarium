import assert from "node:assert/strict";
import test from "node:test";
import type { OperationState } from "../src/api/tasks.ts";
import { latestOperationForModule, operationByTaskId } from "../src/app/operationRegistry.ts";
import { photoCacheIdentity } from "../src/api/photoCacheIdentity.ts";
import { observedSuccessfulCompletion } from "../src/app/operationTransitions.ts";
import { shouldSwitchPhotoLibrary } from "../src/features/settings/photoLibraryUx.ts";
import { describePhotoOperation, photoOperationProgress, photoRenameSummaryFromOperation } from "../src/features/photos/photoOperation.ts";

function operation(overrides: Partial<OperationState> = {}): OperationState {
  return {
    module: "photos",
    task_id: "task-1",
    task_kind: "photo_scan",
    task_scope: "library-a",
    state: "running",
    operation: "photo_scan",
    started_at: "2026-08-09 22:00:00",
    finished_at: null,
    progress: null,
    result: null,
    error: null,
    ...overrides,
  };
}

test("a Photo Library card switches only when it is inactive and the settings view is idle", () => {
  assert.equal(shouldSwitchPhotoLibrary(false, true, false), true);
  assert.equal(shouldSwitchPhotoLibrary(true, true, false), false);
  assert.equal(shouldSwitchPhotoLibrary(false, false, false), false);
  assert.equal(shouldSwitchPhotoLibrary(false, true, true), false);
});

test("photo operation progress presents known totals", () => {
  const value = operation({
    progress: {
      stage: "indexing_photo_metadata",
      current: 1200,
      total: 5000,
      unit: "photos",
    },
  });
  assert.equal(describePhotoOperation(value), "Reading photo metadata: 1,200 / 5,000");
  assert.deepEqual(photoOperationProgress(value), { value: 1200, max: 5000 });
});

test("photo operation progress supports phase-only updates", () => {
  const value = operation();
  assert.equal(describePhotoOperation(value), "Starting");
  assert.equal(photoOperationProgress(value), null);
});

test("photo media cache identity includes the active Photo Library", () => {
  const photo = {
    photo_id: 7,
    directory_id: 2,
    relative_path: "flowers/example.jpg",
    filename: "example.jpg",
    file_size: 4096,
    modified_at_ns: 123456,
    thumbnail_path: null,
  };
  assert.equal(photoCacheIdentity(photo, "library-a"), "library-a:123456:4096");
  assert.notEqual(
    photoCacheIdentity(photo, "library-a"),
    photoCacheIdentity(photo, "library-b"),
  );
});

test("a photo operation completed between recovery snapshots still invalidates photo views", () => {
  const idle = operation({ task_id: null, operation: null, state: "completed" });
  const completed = operation({
    state: "completed",
    finished_at: "2026-08-09 22:00:01",
  });
  assert.equal(observedSuccessfulCompletion(idle, completed), true);
  assert.equal(observedSuccessfulCompletion(completed, completed), false);
  assert.equal(observedSuccessfulCompletion(idle, { ...completed, state: "failed", error: "failed" }), false);
});

test("background status resolves exact tasks without collapsing module history", () => {
  const older = operation({ task_id: "task-1", state: "completed" });
  const newer = operation({ task_id: "task-2", started_at: "2026-08-09 22:01:00" });
  const operations = { "task-1": older, "task-2": newer };
  assert.equal(operationByTaskId(operations, "task-1"), older);
  assert.equal(latestOperationForModule(operations, "photos"), newer);
});

test("an active queued task is preferred over completed module history", () => {
  const completed = operation({
    task_id: "task-complete",
    state: "completed",
    started_at: "2026-08-09 22:02:00",
    finished_at: "2026-08-09 22:03:00",
  });
  const queued = operation({
    task_id: "task-queued",
    state: "queued",
    started_at: null,
    finished_at: null,
  });
  assert.equal(latestOperationForModule({ completed, queued }, "photos"), queued);
});

test("reads the completed bulk rename result from the photo operation", () => {
  const result = {
    operation_id: 12,
    total: 3,
    applied: 2,
    no_change: 1,
    failed: 0,
  };
  assert.deepEqual(
    photoRenameSummaryFromOperation(operation({ state: "completed", result })),
    result,
  );
  assert.throws(
    () => photoRenameSummaryFromOperation(operation({ state: "failed", error: "rename failed" })),
    /rename failed/,
  );
});
