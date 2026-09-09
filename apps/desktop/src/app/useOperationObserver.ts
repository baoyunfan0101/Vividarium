import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import { desktopRuntime } from "../api/client";
import {
  getOperationsStatus,
  type OperationState,
  type OperationsStatus,
} from "../api/tasks";
import { emitPhotoMutation } from "../features/photos/photoMutations";
import { observedSuccessfulCompletion } from "./operationTransitions";

export function useOperationObserver() {
  const [operations, setOperations] = useState<OperationsStatus>({});
  const previous = useRef<OperationsStatus>({});

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    let eventRevision = 0;

    const publish = (operation: OperationState) => {
      if (disposed || !operation.task_id) return;
      const prior = previous.current[operation.task_id];
      publishPhotoMutation(prior, operation);
      const next = { ...previous.current, [operation.task_id]: operation };
      previous.current = next;
      setOperations(next);
    };

    const recover = async () => {
      const revisionAtStart = eventRevision;
      try {
        const recovered = await getOperationsStatus();
        if (disposed) return;
        const next = revisionAtStart === eventRevision
          ? recovered
          : { ...recovered, ...previous.current };
        for (const operation of Object.values(next)) {
          publishPhotoMutation(
            operation.task_id ? previous.current[operation.task_id] : undefined,
            operation,
          );
        }
        previous.current = next;
        setOperations(next);
      } catch {}
    };

    if (desktopRuntime) {
      void listen<OperationState>("operation-progress", (event) => {
        eventRevision += 1;
        publish(event.payload);
      }).then((nextUnlisten) => {
        if (disposed) nextUnlisten();
        else {
          unlisten = nextUnlisten;
          void recover();
        }
      }).catch(() => { void recover(); });
    } else void recover();
    const recoverVisibleState = () => {
      if (document.visibilityState === "visible") void recover();
    };
    document.addEventListener("visibilitychange", recoverVisibleState);
    window.addEventListener("focus", recover);
    return () => {
      disposed = true;
      unlisten?.();
      document.removeEventListener("visibilitychange", recoverVisibleState);
      window.removeEventListener("focus", recover);
    };
  }, []);

  return operations;
}

function publishPhotoMutation(
  prior: OperationState | undefined,
  operation: OperationState,
) {
  const stage = operation.progress?.stage ?? operation.operation ?? operation.module;
  const progressed = operation.state === "running"
    && operation.progress?.current !== prior?.progress?.current;
  if (operation.module === "photos" && progressed) {
    emitPhotoMutation({
      photoId: null,
      kind: stage.toLowerCase().includes("metadata") ? "metadata" : "index",
    });
  }
  if (operation.module === "mapping" && observedSuccessfulCompletion(prior, operation)) {
    emitPhotoMutation({ photoId: null, kind: "mapping" });
  }
  if (operation.module === "photos" && observedSuccessfulCompletion(prior, operation)) {
    emitPhotoMutation({ photoId: null, kind: "photo" });
  }
}
