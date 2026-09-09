import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  acceptedTaxonNameGroup,
  buildTaxonNameGroupSaveInput,
  canDeleteTaxonName,
  canPromoteTaxonName,
  createBlankTaxonNameDraftRow,
  createTaxonNameDraftRows,
  isPrimaryTaxonNameGroup,
  taxonNameGroupKinds,
  taxonNameGroupLabels,
} from "../src/features/taxonomy/taxonEditing.ts";

function source(path: string) {
  return readFileSync(new URL(path, import.meta.url), "utf8");
}

test("defines the six visible groups and their primary relationships", () => {
  assert.deepEqual(taxonNameGroupKinds, [
    "sci_name",
    "synonym",
    "zh_name",
    "zh_alias",
    "en_name",
    "en_alias",
  ]);
  assert.deepEqual(taxonNameGroupLabels, {
    sci_name: "Science name",
    synonym: "Synonyms",
    zh_name: "Chinese name",
    zh_alias: "Chinese aliases",
    en_name: "English name",
    en_alias: "English aliases",
  });
  assert.deepEqual(
    taxonNameGroupKinds.map((kind) => [kind, isPrimaryTaxonNameGroup(kind), acceptedTaxonNameGroup(kind)]),
    [
      ["sci_name", true, "sci_name"],
      ["synonym", false, "sci_name"],
      ["zh_name", true, "zh_name"],
      ["zh_alias", false, "zh_name"],
      ["en_name", true, "en_name"],
      ["en_alias", false, "en_name"],
    ],
  );
});

test("uses family state for accepted-name deletion", () => {
  assert.deepEqual(
    taxonNameGroupKinds.map((kind) => [
      kind,
      canPromoteTaxonName(kind),
      canDeleteTaxonName(kind, false),
      canDeleteTaxonName(kind, true),
    ]),
    [
      ["sci_name", false, false, false],
      ["synonym", true, true, true],
      ["zh_name", false, true, false],
      ["zh_alias", true, true, true],
      ["en_name", false, true, false],
      ["en_alias", true, true, true],
    ],
  );
});

test("renders name actions from independent promote and delete decisions", () => {
  const editor = source("../src/features/taxonomy/TaxonNameGroupEditor.tsx");
  const hierarchy = source("../src/features/taxonomy/TaxonomyHierarchyPage.tsx");
  assert.match(editor, /promoteAllowed \|\| deleteAllowed \? \(\s*<div className="taxonomy-name-actions">/);
  assert.match(editor, /\{deleteAllowed \? \(/);
  assert.match(hierarchy, /zh_name: canDeleteTaxonName\("zh_name", detail\.names\.zh_aliases\.length > 0\)/);
  assert.match(hierarchy, /en_name: canDeleteTaxonName\("en_name", detail\.names\.en_aliases\.length > 0\)/);
  assert.match(hierarchy, /deleteAllowed=\{deleteAllowed\[group\.kind\]\}/);
});

test("creates editable metadata rows while keeping existing names immutable", () => {
  assert.deepEqual(createTaxonNameDraftRows([{
    name_id: 17,
    name: "Canis lupus",
    authority_year: "Linnaeus, 1758",
    source: null,
  }]), [{
    nameId: 17,
    name: "Canis lupus",
    authorityYear: "Linnaeus, 1758",
    source: "",
  }]);
  assert.deepEqual(createBlankTaxonNameDraftRow(), {
    nameId: null,
    name: "",
    authorityYear: "",
    source: "",
  });
});

test("builds a group save request with stable updates and normalized additions", () => {
  assert.deepEqual(buildTaxonNameGroupSaveInput(42, "synonym", [
    {
      nameId: 17,
      name: "Canis lycaon",
      authorityYear: "  Schreber, 1775 ",
      source: " ",
    },
    {
      nameId: null,
      name: "  Canis familiaris  ",
      authorityYear: " Linnaeus, 1758 ",
      source: " Catalogue ",
    },
  ]), {
    taxon_id: 42,
    name_type: "synonym",
    updates: [{
      name_id: 17,
      authority_year: "Schreber, 1775",
      source: null,
    }],
    additions: [{
      name: "Canis familiaris",
      authority_year: "Linnaeus, 1758",
      source: "Catalogue",
    }],
  });
});

test("rejects a blank new name only when the group is saved", () => {
  assert.throws(
    () => buildTaxonNameGroupSaveInput(42, "en_name", [createBlankTaxonNameDraftRow()]),
    /Name is required/,
  );
});
