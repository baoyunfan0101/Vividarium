import { call } from "./client";
import { operationByTaskId } from "../app/operationRegistry";

export type OperationProgress = {
  stage: string;
  current: number | null;
  total: number | null;
  unit: "items" | "files" | "photos" | "names" | "taxa" | "bytes" | "statements" | null;
};

export type OperationState = {
  module: string;
  task_id: string | null;
  task_kind: string | null;
  task_scope: string | null;
  state: "queued" | "running" | "completed" | "failed";
  operation: string | null;
  started_at: string | null;
  finished_at: string | null;
  progress: OperationProgress | null;
  result: unknown;
  error: string | null;
};

export type OperationsStatus = Record<string, OperationState>;

export function demoOperation(module: string, _message: string): OperationState {
  return {
    module,
    task_id: null,
    task_kind: null,
    task_scope: null,
    state: "completed",
    operation: null,
    started_at: null,
    finished_at: null,
    progress: null,
    result: null,
    error: null,
  };
}

export function demoCompletedOperation(
  module: string,
  operation: string,
  result: unknown,
): OperationState {
  return {
    ...demoOperation(module, "Complete"),
    operation,
    result,
  };
}

export const getOperationsStatus = () =>
  call<OperationsStatus>("get_operations_status", undefined, () => ({}));

export async function waitForOperation(
  taskId: string,
  onChange?: (operation: OperationState) => void,
): Promise<OperationState> {
  while (true) {
    const operation = operationByTaskId(await getOperationsStatus(), taskId);
    if (!operation || operation.task_id !== taskId) {
      throw new Error(`Background task ${taskId} is unavailable`);
    }
    onChange?.(operation);
    if (!["queued", "running"].includes(operation.state)) return operation;
    await new Promise((resolve) => window.setTimeout(resolve, 250));
  }
}
