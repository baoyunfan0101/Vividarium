import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

function source(path: string) {
  return readFileSync(new URL(path, import.meta.url), "utf8");
}

const view = source("../src/features/operations/OperationHistoryView.tsx");
const api = source("../src/api/operations.ts");

test("taxonomy history loads operation-scoped source input", () => {
  assert.match(api, /export type OperationInput =/);
  assert.match(api, /kind: "custom_sql"; sql: string/);
  assert.match(api, /kind: "formatted_update"; rows: TaxonInputRow\[\]/);
  assert.match(api, /call<OperationInput \| null>\("get_taxonomy_operation_input"/);
  assert.match(view, /getTaxonomyOperationInput\(operation\.operation_id\)/);
});

test("Custom SQL history renders exact input in a read-only SQL editor", () => {
  assert.match(view, /input\.kind === "custom_sql"/);
  assert.match(view, /ariaLabel="Custom SQL operation input"/);
  assert.match(view, /language="sql"[\s\S]*readOnly[\s\S]*value=\{input\.sql\}/);
  assert.match(view, /hideJson=\{operation\.source === "custom_sql"\}/);
  assert.match(view, /row\.entity_id \? ` \$\{row\.entity_id\}`/);
});

test("Formatted Update and legacy history inputs have source-aware presentations", () => {
  assert.match(view, /input\.kind === "formatted_update"/);
  for (const column of [
    "kingdom",
    "order",
    "family",
    "genus",
    "species",
    "authority_year",
    "synonyms",
    "zh_name",
    "zh_alias",
    "en_name",
    "en_alias",
    "geological_range",
    "source",
  ]) {
    assert.match(view, new RegExp(`"${column}"`));
  }
  assert.match(view, /Input is not available for this historical operation\./);
});

test("history keeps Changes and meaningful Before and After audit details", () => {
  assert.match(view, /<h2>Changes<\/h2>/);
  assert.match(view, /<b>Before<\/b>/);
  assert.match(view, /<b>After<\/b>/);
  assert.match(view, /setError\(errorMessage\(nextError\)\)/);
});
