import assert from "node:assert/strict";
import test from "node:test";
import { formatTaxonName } from "../src/features/taxonomy/taxonNameFormatting.ts";

const names = {
  sci_name: "Panthera leo",
  zh_name: "Lion Chinese",
  en_name: "Lion",
};

test("formats each accepted taxon name independently", () => {
  assert.equal(formatTaxonName(names, { sci_name: true, zh_name: false, en_name: false }), "Panthera leo");
  assert.equal(formatTaxonName(names, { sci_name: false, zh_name: true, en_name: false }), "Lion Chinese");
  assert.equal(formatTaxonName(names, { sci_name: false, zh_name: false, en_name: true }), "Lion");
});

test("formats multiple names in setting order with exact separators", () => {
  assert.equal(
    formatTaxonName(names, { sci_name: true, zh_name: true, en_name: true }),
    "Panthera leo \u00b7 Lion Chinese \u00b7 Lion",
  );
});

test("skips missing selected names without empty separators", () => {
  assert.equal(
    formatTaxonName(
      { sci_name: "Panthera leo", zh_name: null, en_name: "Lion" },
      { sci_name: true, zh_name: true, en_name: true },
    ),
    "Panthera leo \u00b7 Lion",
  );
  assert.equal(
    formatTaxonName(
      { sci_name: null, zh_name: null, en_name: null },
      { sci_name: true, zh_name: true, en_name: true },
      "Taxon 7",
    ),
    "Taxon 7",
  );
});
