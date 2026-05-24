import { useRef, useState, type PointerEvent as ReactPointerEvent } from "react";

export function useResizableSplit(initialRatio: number) {
  const [splitRatio, setSplitRatio] = useState(initialRatio);
  const splitRef = useRef<HTMLElement | null>(null);

  function beginResize(event: ReactPointerEvent<HTMLDivElement>) {
    event.preventDefault();
    const element = splitRef.current;
    if (!element) {
      return;
    }
    const rect = element.getBoundingClientRect();

    function update(clientX: number) {
      const nextRatio = ((clientX - rect.left) / rect.width) * 100;
      setSplitRatio(Math.min(Math.max(nextRatio, 22), 68));
    }

    function handleMove(moveEvent: PointerEvent) {
      update(moveEvent.clientX);
    }

    function handleUp() {
      window.removeEventListener("pointermove", handleMove);
      document.body.classList.remove("is-resizing-split");
    }

    document.body.classList.add("is-resizing-split");
    window.addEventListener("pointermove", handleMove);
    window.addEventListener("pointerup", handleUp, { once: true });
    update(event.clientX);
  }

  return { beginResize, splitRatio, splitRef };
}
