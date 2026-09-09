import type { PhotoRenameOperationSummary } from "../../api/photos";
import type { OperationState } from "../../api/tasks";
import { backgroundStageLabel } from "../../app/backgroundPresentation.ts";

export function describePhotoOperation(operation: OperationState): string {
  const stage = backgroundStageLabel(operation);
  const current = operation.progress?.current ?? 0;
  const total = operation.progress?.total ?? null;
  if (total === null || total <= 0) return stage;
  return `${stage}: ${current.toLocaleString("en-US")} / ${total.toLocaleString("en-US")}`;
}

export function photoOperationProgress(operation: OperationState): { value: number; max: number } | null {
  const value = operation.progress?.current ?? 0;
  const max = operation.progress?.total ?? null;
  if (max === null || max <= 0) return null;
  return { value: Math.min(value, max), max };
}

export function photoRenameSummaryFromOperation(
  operation: OperationState,
): PhotoRenameOperationSummary {
  if (operation.error) throw new Error(operation.error);
  const result = operation.result as Partial<PhotoRenameOperationSummary> | null;
  if (
    !result
    || typeof result.total !== "number"
    || typeof result.applied !== "number"
    || typeof result.no_change !== "number"
    || typeof result.failed !== "number"
  ) {
    throw new Error("Photo rename operation did not return a result");
  }
  return {
    operation_id: typeof result.operation_id === "number" ? result.operation_id : null,
    total: result.total,
    applied: result.applied,
    no_change: result.no_change,
    failed: result.failed,
  };
}

export function formatPhotoRenameSummary(summary: PhotoRenameOperationSummary): string {
  return `Renamed ${summary.applied} photos, ${summary.no_change} unchanged, ${summary.failed} failed`;
}
