import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { selectionIntersectsElement } from "../src/shared/selectableSurface.ts";

function source(path: string) {
  return readFileSync(new URL(path, import.meta.url), "utf8");
}

function selection(intersections: boolean[], isCollapsed = false) {
  return {
    isCollapsed,
    rangeCount: intersections.length,
    getRangeAt: (index: number) => ({
      intersectsNode: () => intersections[index],
    }),
  } as unknown as Selection;
}

test("collapsed selection allows activation", () => {
  assert.equal(
    selectionIntersectsElement({} as HTMLElement, selection([true], true)),
    false,
  );
});

test("selection inside the current surface suppresses activation", () => {
  assert.equal(selectionIntersectsElement({} as HTMLElement, selection([true])), true);
});

test("a range crossing the current surface suppresses activation", () => {
  assert.equal(selectionIntersectsElement({} as HTMLElement, selection([false, true])), true);
});

test("selection in another surface allows activation", () => {
  assert.equal(selectionIntersectsElement({} as HTMLElement, selection([false])), false);
});

test("major content surfaces use selection-aware non-button containers", () => {
  const card = source("../src/features/taxonomy/TaxonCard.tsx");
  const hierarchy = source("../src/features/taxonomy/TaxonomyHierarchyPage.tsx");
  const browser = source("../src/features/photos/PhotoBrowser.tsx");
  const photos = source("../src/features/photos/PhotosView.tsx");
  const mapping = source("../src/features/mapping/MappingView.tsx");
  const operations = source("../src/features/operations/OperationHistoryView.tsx");

  assert.match(card, /className="taxon-card-main selectable-content"/);
  assert.doesNotMatch(card, /<button className="taxon-card-main"/);
  assert.match(hierarchy, /className="taxonomy-child-button selectable-content"/);
  assert.match(browser, /className=\{`photo-list-row selectable-content/);
  assert.match(photos, /className="tree-node-content selectable-content"/);
  assert.match(photos, /className=\{`finder-row selectable-content/);
  assert.match(mapping, /className=\{`mapping-photo-row selectable-content/);
  assert.match(operations, /className="operation-summary-main selectable-content"/);
  for (const content of [card, hierarchy, browser, photos, mapping, operations]) {
    assert.match(content, /selectionIntersectsElement/);
  }
});

test("command controls and selectable surfaces have distinct CSS contracts", () => {
  const theme = source("../src/styles/theme.css");
  const shared = source("../src/styles/shared.css");
  assert.match(theme, /button, \.button \{[\s\S]*?user-select: none;[\s\S]*?-webkit-user-select: none;/);
  assert.doesNotMatch(theme, /\[role=["']button["']\][^{]*\{[^}]*user-select: none/);
  assert.match(shared, /\.selectable-content \{[\s\S]*?user-select: text;[\s\S]*?-webkit-user-select: text;/);
});
