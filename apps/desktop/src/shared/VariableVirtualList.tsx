import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type UIEvent,
} from "react";
import { useViewState } from "./viewState";
import {
  buildVariableRowLayout,
  reconcileVariableRowMeasurements,
  variableVisibleRange,
  type VariableRowKey,
  type VariableRowLayout,
} from "./variableVirtualLayout";

type VariableVirtualListProps<T> = {
  items: T[];
  estimatedRowHeight?: number;
  className?: string;
  overscan?: number;
  itemKey: (item: T, index: number) => VariableRowKey;
  renderItem: (item: T, index: number) => ReactNode;
  onNearEnd?: () => void;
  resetKey?: string | number | boolean | null;
  stateKey?: string;
};

function MeasuredVariableRow({
  rowKey,
  offset,
  onMeasure,
  children,
}: {
  rowKey: VariableRowKey;
  offset: number;
  onMeasure: (key: VariableRowKey, height: number) => void;
  children: ReactNode;
}) {
  const rowRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const element = rowRef.current;
    if (!element) return;
    const measure = () => onMeasure(rowKey, element.getBoundingClientRect().height);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [onMeasure, rowKey]);
  return (
    <div
      className="virtual-row variable-virtual-row"
      ref={rowRef}
      style={{ transform: `translateY(${offset}px)` }}
    >
      {children}
    </div>
  );
}

export function VariableVirtualList<T>({
  items,
  estimatedRowHeight = 90,
  className = "",
  overscan = 8,
  itemKey,
  renderItem,
  onNearEnd,
  resetKey,
  stateKey,
}: VariableVirtualListProps<T>) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const measurementsRef = useRef(new Map<VariableRowKey, number>());
  const measurementResetKeyRef = useRef(resetKey);
  const keysRef = useRef<VariableRowKey[]>([]);
  const layoutRef = useRef<VariableRowLayout>({ offsets: [], heights: [], totalHeight: 0 });
  const [measurementVersion, setMeasurementVersion] = useState(0);
  const [height, setHeight] = useState(0);
  const [scrollTop, setScrollTop] = useViewState(
    stateKey ? `${stateKey}.scroll-top` : null,
    0,
  );
  const keys = items.map(itemKey);
  const resetMeasurements = !Object.is(measurementResetKeyRef.current, resetKey);
  measurementResetKeyRef.current = resetKey;
  measurementsRef.current = reconcileVariableRowMeasurements(
    measurementsRef.current,
    keys,
    resetMeasurements,
  );
  const layout = buildVariableRowLayout(keys, estimatedRowHeight, measurementsRef.current);
  keysRef.current = keys;
  layoutRef.current = layout;
  const range = variableVisibleRange(layout, scrollTop, height, overscan);

  useEffect(() => {
    const element = viewportRef.current;
    if (!element) return;
    element.scrollTop = scrollTop;
    const update = () => setHeight(element.clientHeight);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const element = viewportRef.current;
    if (!element) return;
    const maximum = Math.max(0, layout.totalHeight - element.clientHeight);
    const nextScrollTop = resetMeasurements ? 0 : Math.min(Math.max(0, scrollTop), maximum);
    if (element.scrollTop !== nextScrollTop) element.scrollTop = nextScrollTop;
    if (scrollTop !== nextScrollTop) setScrollTop(nextScrollTop);
  }, [height, layout.totalHeight, measurementVersion, resetMeasurements, scrollTop, setScrollTop]);

  const measureRow = useCallback((key: VariableRowKey, nextHeight: number) => {
    if (!Number.isFinite(nextHeight) || nextHeight <= 0) return;
    const previousHeight = measurementsRef.current.get(key) ?? estimatedRowHeight;
    if (Math.abs(previousHeight - nextHeight) < 0.5) return;
    const rowIndex = keysRef.current.indexOf(key);
    const element = viewportRef.current;
    if (element && rowIndex >= 0) {
      const anchor = variableVisibleRange(
        layoutRef.current,
        element.scrollTop,
        element.clientHeight,
        0,
      ).start;
      if (rowIndex < anchor) {
        const anchoredScrollTop = Math.max(0, element.scrollTop + nextHeight - previousHeight);
        element.scrollTop = anchoredScrollTop;
        setScrollTop(anchoredScrollTop);
      }
    }
    measurementsRef.current.set(key, nextHeight);
    setMeasurementVersion((current) => current + 1);
  }, [estimatedRowHeight, setScrollTop]);

  function handleScroll(event: UIEvent<HTMLDivElement>) {
    const element = event.currentTarget;
    setScrollTop(element.scrollTop);
    if (layout.totalHeight - element.scrollTop - element.clientHeight < estimatedRowHeight * 5) {
      onNearEnd?.();
    }
  }

  return (
    <div
      className={`virtual-viewport ${className}`}
      ref={viewportRef}
      onScroll={handleScroll}
      tabIndex={0}
    >
      <div className="virtual-spacer" style={{ height: layout.totalHeight }}>
        <div className="virtual-window variable-virtual-window">
          {items.slice(range.start, range.end).map((item, offset) => {
            const index = range.start + offset;
            const key = keys[index];
            return (
              <MeasuredVariableRow
                key={key}
                rowKey={key}
                offset={layout.offsets[index]}
                onMeasure={measureRow}
              >
                {renderItem(item, index)}
              </MeasuredVariableRow>
            );
          })}
        </div>
      </div>
    </div>
  );
}
