import { useEffect, useState } from "react";
import type { OperationState, OperationsStatus } from "../api/tasks";
import {
  backgroundElapsed,
  backgroundProgressMode,
  backgroundProgressText,
  backgroundStageLabel,
  backgroundTaskSections,
  backgroundTaskTitle,
} from "./backgroundPresentation";

export function BackgroundTasks({ operations }: { operations: OperationsStatus }) {
  const [now, setNow] = useState(Date.now());
  const sections = backgroundTaskSections(operations);
  const running = sections.active.some((operation) => operation.state === "running");

  useEffect(() => {
    if (!running) return undefined;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [running]);

  return (
    <div className="toolbar-popover background-popover">
      <strong>Background</strong>
      {sections.active.length === 0 && sections.recent.length === 0 && (
        <span className="background-empty">No background tasks</span>
      )}
      {sections.active.length > 0 && (
        <section className="background-section">
          <h3>Active</h3>
          {sections.active.map((operation) => (
            <BackgroundTask key={taskKey(operation)} operation={operation} now={now} />
          ))}
        </section>
      )}
      {sections.recent.length > 0 && (
        <section className="background-section background-section-recent">
          <h3>Recent</h3>
          {sections.recent.map((operation) => (
            <BackgroundTask key={taskKey(operation)} operation={operation} now={now} />
          ))}
        </section>
      )}
    </div>
  );
}

function BackgroundTask({ operation, now }: { operation: OperationState; now: number }) {
  const mode = backgroundProgressMode(operation);
  const elapsed = backgroundElapsed(operation, now);
  const count = backgroundProgressText(operation);
  const progress = operation.progress;
  return (
    <article className={`background-task background-task-${operation.state}`}>
      <div className="background-task-header">
        <strong className="background-task-title">{backgroundTaskTitle(operation)}</strong>
        {elapsed && <span className="background-task-time">{elapsed}</span>}
      </div>
      <span className="background-task-stage">{backgroundStageLabel(operation)}</span>
      {mode === "determinate" && progress?.current != null && progress.total != null && (
        <progress
          className="background-task-progress"
          value={Math.min(progress.current, Math.max(progress.total, 0))}
          max={Math.max(progress.total, 1)}
        />
      )}
      {mode === "indeterminate" && <progress className="background-task-progress" />}
      {count && <span className="background-task-count">{count}</span>}
      {operation.error && <span className="background-task-error">{operation.error}</span>}
    </article>
  );
}

function taskKey(operation: OperationState): string {
  return operation.task_id ?? `${operation.module}:${operation.operation}:${operation.started_at}`;
}
