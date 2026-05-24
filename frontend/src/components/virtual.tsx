import { useEffect, useRef, useState, type MutableRefObject, type ReactNode, type Ref, type UIEvent } from "react";

function assignRef<T>(ref: Ref<T> | undefined, value: T | null) {
  if (!ref) {
    return;
  }
  if (typeof ref === "function") {
    ref(value);
    return;
  }
  (ref as MutableRefObject<T | null>).current = value;
}

export function VirtualList({
  itemCount,
  itemHeight,
  className,
  overscan = 12,
  scrollRef,
  onScroll,
  itemKey,
  renderItem,
}: {
  itemCount: number;
  itemHeight: number;
  className?: string;
  overscan?: number;
  scrollRef?: Ref<HTMLDivElement>;
  onScroll?: (event: UIEvent<HTMLDivElement>) => void;
  itemKey: (index: number) => string | number;
  renderItem: (index: number) => ReactNode;
}) {
  const localRef = useRef<HTMLDivElement | null>(null);
  const [viewportHeight, setViewportHeight] = useState(0);
  const [scrollTop, setScrollTop] = useState(0);

  useEffect(() => {
    const element = localRef.current;
    if (!element) {
      return;
    }
    function updateHeight() {
      if (element) {
        setViewportHeight(element.clientHeight);
        setScrollTop(element.scrollTop);
      }
    }
    updateHeight();
    const observer = new ResizeObserver(updateHeight);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  function setRef(element: HTMLDivElement | null) {
    localRef.current = element;
    assignRef(scrollRef, element);
    if (element) {
      window.requestAnimationFrame(() => {
        setViewportHeight(element.clientHeight);
        setScrollTop(element.scrollTop);
      });
    }
  }

  function handleScroll(event: UIEvent<HTMLDivElement>) {
    setScrollTop(event.currentTarget.scrollTop);
    onScroll?.(event);
  }

  const startIndex = Math.max(0, Math.floor(scrollTop / itemHeight) - overscan);
  const endIndex = Math.min(
    itemCount,
    Math.ceil((scrollTop + viewportHeight) / itemHeight) + overscan,
  );
  const indexes = [];
  for (let index = startIndex; index < endIndex; index += 1) {
    indexes.push(index);
  }

  return (
    <div className={className} ref={setRef} onScroll={handleScroll}>
      <div className="virtual-list-spacer" style={{ height: itemCount * itemHeight }}>
        <div className="virtual-list-window" style={{ transform: `translateY(${startIndex * itemHeight}px)` }}>
          {indexes.map((index) => (
            <div className="virtual-list-row" key={itemKey(index)}>
              {renderItem(index)}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
