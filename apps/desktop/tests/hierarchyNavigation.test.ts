import assert from "node:assert/strict";
import test from "node:test";
import {
  createHierarchyNavigationState,
  currentTaxonForRoot,
  hierarchyNavigationReducer,
  reconcileSelectedRoot,
  recordHierarchyPosition,
  taxonSearchMatchExplanations,
} from "../src/features/taxonomy/hierarchyNavigation.ts";
import {
  taxonMatchExplanations,
  type TaxonMatchExplanation,
} from "../src/features/taxonomy/taxonMatchExplanation.ts";

function explanationText(explanations: TaxonMatchExplanation[]) {
  return explanations.map((explanation) => `${explanation.label} · ${explanation.name}`);
}

test("stores an independent current taxon for each search root", () => {
  let positions = {};
  assert.equal(currentTaxonForRoot(10, positions), 10);
  positions = recordHierarchyPosition(positions, 10, 11);
  assert.equal(currentTaxonForRoot(20, positions), 20);
  positions = recordHierarchyPosition(positions, 20, 21);
  assert.equal(currentTaxonForRoot(10, positions), 11);
  assert.equal(currentTaxonForRoot(20, positions), 21);
});

test("reconciles the selected root without resetting a surviving result", () => {
  assert.equal(reconcileSelectedRoot(20, [10, 20, 30]), 20);
  assert.equal(reconcileSelectedRoot(20, [10, 30]), 10);
  assert.equal(reconcileSelectedRoot(null, [10, 30]), 10);
  assert.equal(reconcileSelectedRoot(20, []), null);
});

test("loads children only after expansion and resets them on navigation", () => {
  let state = createHierarchyNavigationState(10);
  assert.equal(state.childrenExpanded, false);
  assert.equal(state.childrenRequested, false);
  state = hierarchyNavigationReducer(state, { type: "toggle-children" });
  assert.equal(state.childrenExpanded, true);
  assert.equal(state.childrenRequested, true);
  state = hierarchyNavigationReducer(state, { type: "toggle-children" });
  assert.equal(state.childrenExpanded, false);
  assert.equal(state.childrenRequested, true);
  state = hierarchyNavigationReducer(state, { type: "navigate", taxonId: 11 });
  assert.deepEqual(state, createHierarchyNavigationState(11));
  state = hierarchyNavigationReducer(
    hierarchyNavigationReducer(state, { type: "toggle-children" }),
    { type: "reset", taxonId: 11 },
  );
  assert.deepEqual(state, createHierarchyNavigationState(11));
});

test("taxonomy search explains non-accepted matches alongside accepted matches", () => {
  const result = {
    taxon_id: 10,
    rank: "species" as const,
    names: { sci_name: "Canis lupus", zh_name: null, en_name: "Wolf" },
    matches: [{ name_id: 2, name_type: "synonym" as const, name: "Canis lycaon" }],
  };
  assert.deepEqual(explanationText(taxonSearchMatchExplanations(result)), [
    "Matched synonym · Canis lycaon",
  ]);
  assert.deepEqual(taxonSearchMatchExplanations({
    ...result,
    matches: [{ name_id: 1, name_type: "sci_name", name: "Canis lupus" }],
  }), []);
  assert.deepEqual(explanationText(taxonSearchMatchExplanations({
    ...result,
    matches: [
      { name_id: 1, name_type: "sci_name", name: "Canis lupus" },
      { name_id: 2, name_type: "synonym", name: "Canis lycaon" },
    ],
  })), ["Matched synonym · Canis lycaon"]);
});

test("shared match explanations cover aliases, deduplication, and deterministic order", () => {
  assert.deepEqual(taxonMatchExplanations([
    { name_type: "sci_name", name: "Panthera leo" },
    { name_type: "zh_name", name: "lion" },
    { name_type: "en_name", name: "Lion" },
  ]), []);
  assert.deepEqual(explanationText(taxonMatchExplanations([
    { name_type: "zh_alias", name: "old lion" },
  ])), ["Matched Chinese alias · old lion"]);
  assert.deepEqual(explanationText(taxonMatchExplanations([
    { name_type: "en_alias", name: "Cave lion" },
  ])), ["Matched English alias · Cave lion"]);
  assert.deepEqual(explanationText(taxonMatchExplanations([
    { name_type: "en_alias", name: "Cave lion" },
    { name_type: "sci_name", name: "Panthera leo" },
    { name_type: "zh_alias", name: "old lion" },
    { name_type: "synonym", name: "Felis leo" },
    { name_type: "synonym", name: "Leo leo" },
    { name_type: "synonym", name: "Felis leo" },
    { name_type: "en_alias", name: "Cave lion" },
  ])), [
    "Matched synonym · Felis leo",
    "Matched synonym · Leo leo",
    "Matched Chinese alias · old lion",
    "Matched English alias · Cave lion",
  ]);
});
