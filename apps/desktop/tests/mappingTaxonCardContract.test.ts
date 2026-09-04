import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

function source(path: string) {
  return readFileSync(new URL(path, import.meta.url), "utf8");
}

test("mapping editor uses one detail read model and shared match explanations", () => {
  const editor = source("../src/features/mapping/MappingEditor.tsx");
  const api = source("../src/api/mapping.ts");
  assert.match(editor, /const nextMatch = await getPhotoMappingDetail\(photo\.photo_id\)/);
  assert.doesNotMatch(editor, /getPhotoMappingCandidates|getPhotoMapping\(/);
  assert.match(editor, /description=\{taxonMatchExplanation\(match\.matched_names\)\}/);
  assert.match(editor, /description=\{taxonMatchExplanation\(candidate\.matched_names\)\}/);
  assert.match(editor, /description=\{taxonMatchExplanation\(item\.matches\)\}/);
  assert.match(api, /matched_names: PhotoMatchedName\[\]/);
  assert.doesNotMatch(api, /get_photo_mapping_candidates|getPhotoMappingCandidates/);
});

test("current, candidate, and search cards open taxon detail", () => {
  const editor = source("../src/features/mapping/MappingEditor.tsx");
  assert.match(editor, /onClick=\{\(\) => handlers\.openTaxon\(currentTaxon\.taxon_id\)\}/);
  assert.match(editor, /onClick=\{\(\) => handlers\.openTaxon\(candidate\.summary\.taxon_id\)\}/);
  assert.match(editor, /onClick=\{\(\) => handlers\.openTaxon\(item\.taxon_id\)\}/);
});

test("TaxonCard allows text selection without triggering navigation", () => {
  const card = source("../src/features/taxonomy/TaxonCard.tsx");
  const styles = source("../src/styles/shared.css");
  assert.match(styles, /\.selectable-content \{[\s\S]*?user-select: text;/);
  assert.match(card, /className="taxon-card-main selectable-content"/);
  assert.match(card, /selectionIntersectsElement\(event\.currentTarget\)/);
  assert.match(card, /role=\{onClick \? "button" : undefined\}/);
  assert.match(card, /event\.key !== "Enter" && event\.key !== " "/);
});

test("compact mapping cards reserve four-line vertical space", () => {
  const editor = source("../src/features/mapping/MappingEditor.tsx");
  const styles = source("../src/styles/taxonomy.css");
  assert.match(styles, /\.taxon-card\.compact \.taxon-card-main \{[^}]*padding-block: 4px;/);
  assert.match(editor, /className="candidate-stack"[\s\S]*?rowHeight=\{60\}/);
  assert.match(editor, /className="mapping-search-results"[\s\S]*?rowHeight=\{60\}/);
});

test("MappingEditor documentation describes its current handler interface", () => {
  const documentation = source("../../../docs/frontend/MAPPING.md");
  assert.match(documentation, /`handlers`: shared `PhotoOpenHandlers`/);
  assert.doesNotMatch(documentation, /`onOpenTaxon`/);
});
