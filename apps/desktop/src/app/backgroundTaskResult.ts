import type { OperationState } from "../api/tasks.ts";

export function operationResult<T>(operation: OperationState, taskId: string | null): T {
  if (taskId !== null && operation.task_id !== taskId) {
    throw new Error(`Background task ${taskId} returned a different task result`);
  }
  if (operation.error) throw new Error(operation.error);
  if (operation.state !== "completed") {
    throw new Error(`Background task did not complete: ${operation.state}`);
  }
  if (operation.result === null) throw new Error("Background task completed without a result");
  return operation.result as T;
}
