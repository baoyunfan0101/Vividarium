import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  buildVariableRowLayout,
  reconcileVariableRowMeasurements,
  variableVisibleRange,
} from "../src/shared/variableVirtualLayout.ts";

function source(path: string) {
  return readFileSync(new URL(path, import.meta.url), "utf8");
}

test("unmeasured variable rows use equal estimates", () => {
  const layout = buildVariableRowLayout(["a", "b", "c"], 90, new Map());
  assert.deepEqual(layout.offsets, [0, 90, 180]);
  assert.deepEqual(layout.heights, [90, 90, 90]);
  assert.equal(layout.totalHeight, 270);
});

test("mixed measurements determine offsets and total height", () => {
  const layout = buildVariableRowLayout(
    ["a", "b", "c"],
    90,
    new Map([["a", 60], ["b", 120]]),
  );
  assert.deepEqual(layout.offsets, [0, 60, 180]);
  assert.deepEqual(layout.heights, [60, 120, 90]);
  assert.equal(layout.totalHeight, 270);
});

test("measurement updates change later offsets, totals, and the visible range", () => {
  const initial = buildVariableRowLayout(["a", "b", "c", "d"], 90, new Map());
  const updated = buildVariableRowLayout(
    ["a", "b", "c", "d"],
    90,
    new Map([["b", 150]]),
  );
  assert.deepEqual(initial.offsets, [0, 90, 180, 270]);
  assert.deepEqual(updated.offsets, [0, 90, 240, 330]);
  assert.equal(updated.totalHeight, 420);
  assert.deepEqual(variableVisibleRange(initial, 185, 80, 0), { start: 2, end: 3 });
  assert.deepEqual(variableVisibleRange(updated, 185, 80, 0), { start: 1, end: 3 });
});

test("measurements follow stable item keys when items reorder", () => {
  const measurements = new Map<string | number, number>([["a", 60], ["b", 120]]);
  const reconciled = reconcileVariableRowMeasurements(measurements, ["b", "a", "c"], false);
  const layout = buildVariableRowLayout(["b", "a", "c"], 90, reconciled);
  assert.deepEqual(layout.heights, [120, 60, 90]);
  assert.deepEqual(layout.offsets, [0, 120, 180]);
});

test("a reset drops incompatible measurements", () => {
  const measurements = new Map<string | number, number>([["a", 60], ["b", 120]]);
  const reset = reconcileVariableRowMeasurements(measurements, ["a", "b"], true);
  assert.equal(reset.size, 0);
  assert.deepEqual(buildVariableRowLayout(["a", "b"], 90, reset).heights, [90, 90]);
});

test("taxonomy and mapping match lists use the shared variable-height component", () => {
  const taxonomy = source("../src/features/taxonomy/TaxonomyView.tsx");
  const mapping = source("../src/features/mapping/MappingEditor.tsx");
  const taxonomyResults = taxonomy.slice(
    taxonomy.indexOf("const resultsPane"),
    taxonomy.indexOf("const recordsPane"),
  );
  assert.match(taxonomyResults, /<VariableVirtualList/);
  assert.match(taxonomyResults, /estimatedRowHeight=\{90\}/);
  assert.equal((mapping.match(/<VariableVirtualList/g) ?? []).length, 2);
  assert.equal((mapping.match(/estimatedRowHeight=\{90\}/g) ?? []).length, 2);
  assert.doesNotMatch(taxonomyResults, /rowHeight=\{90\}/);
  assert.doesNotMatch(mapping, /rowHeight=\{90\}/);
});
