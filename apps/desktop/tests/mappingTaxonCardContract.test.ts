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
  assert.match(editor, /matchExplanations=\{taxonMatchExplanations\(match\.matched_names\)\}/);
  assert.match(editor, /matchExplanations=\{taxonMatchExplanations\(candidate\.matched_names\)\}/);
  assert.match(editor, /matchExplanations=\{taxonMatchExplanations\(item\.matches\)\}/);
  assert.match(api, /matched_names: PhotoMatchedName\[\]/);
  assert.doesNotMatch(api, /get_photo_mapping_candidates|getPhotoMappingCandidates/);
});

test("TaxonCard renders each typed match explanation as selectable card content", () => {
  const card = source("../src/features/taxonomy/TaxonCard.tsx");
  assert.match(card, /matchExplanations\.map\(\(explanation\) =>/);
  assert.match(card, /\{explanation\.label\} · \{explanation\.name\}/);
  assert.doesNotMatch(card, /description\?: string/);
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

test("compact mapping cards reserve space for multiple match explanations", () => {
  const editor = source("../src/features/mapping/MappingEditor.tsx");
  const styles = source("../src/styles/taxonomy.css");
  assert.match(styles, /\.taxon-card\.compact \.taxon-card-main \{[^}]*padding-block: 4px;/);
  assert.match(editor, /<VariableVirtualList[\s\S]*?className="candidate-stack"/);
  assert.match(editor, /<VariableVirtualList[\s\S]*?className="mapping-search-results"/);
  assert.equal((editor.match(/estimatedRowHeight=\{90\}/g) ?? []).length, 2);
});

test("MappingEditor documentation describes its current handler interface", () => {
  const documentation = source("../../../docs/frontend/MAPPING.md");
  assert.match(documentation, /`handlers`: shared `PhotoOpenHandlers`/);
  assert.doesNotMatch(documentation, /`onOpenTaxon`/);
});
