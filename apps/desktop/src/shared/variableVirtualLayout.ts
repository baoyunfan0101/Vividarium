export type VariableRowKey = string | number;

export type VariableRowLayout = {
  offsets: number[];
  heights: number[];
  totalHeight: number;
};

export function buildVariableRowLayout(
  keys: readonly VariableRowKey[],
  estimatedRowHeight: number,
  measurements: ReadonlyMap<VariableRowKey, number>,
): VariableRowLayout {
  const estimate = Math.max(1, estimatedRowHeight);
  const offsets: number[] = [];
  const heights: number[] = [];
  let totalHeight = 0;
  for (const key of keys) {
    offsets.push(totalHeight);
    const height = Math.max(1, measurements.get(key) ?? estimate);
    heights.push(height);
    totalHeight += height;
  }
  return { offsets, heights, totalHeight };
}

export function variableVisibleRange(
  layout: VariableRowLayout,
  scrollTop: number,
  viewportHeight: number,
  overscan: number,
): { start: number; end: number } {
  const itemCount = layout.offsets.length;
  if (itemCount === 0) return { start: 0, end: 0 };
  const viewportTop = Math.max(0, scrollTop);
  const viewportBottom = viewportTop + Math.max(0, viewportHeight);
  let first = 0;
  while (
    first < itemCount
    && layout.offsets[first] + layout.heights[first] <= viewportTop
  ) {
    first += 1;
  }
  let last = first;
  while (last < itemCount && layout.offsets[last] < viewportBottom) last += 1;
  const margin = Math.max(0, Math.floor(overscan));
  return {
    start: Math.max(0, first - margin),
    end: Math.min(itemCount, Math.max(first + 1, last) + margin),
  };
}

export function reconcileVariableRowMeasurements(
  measurements: ReadonlyMap<VariableRowKey, number>,
  keys: readonly VariableRowKey[],
  reset: boolean,
): Map<VariableRowKey, number> {
  if (reset) return new Map();
  const next = new Map<VariableRowKey, number>();
  for (const key of keys) {
    const height = measurements.get(key);
    if (height !== undefined) next.set(key, height);
  }
  return next;
}
