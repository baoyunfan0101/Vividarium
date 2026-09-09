import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

function source(path: string) {
  return readFileSync(new URL(path, import.meta.url), "utf8");
}

test("taxon tree reports direct entry counts through the tab status", () => {
  const photos = source("../src/features/photos/PhotosView.tsx");
  const taxonTree = photos.slice(photos.indexOf("export function TaxonPhotosView"), photos.indexOf("export function PhotoMapView"));
  assert.match(taxonTree, /onStatus: \(message: string\) => void;/);
  assert.match(taxonTree, /getPhotoTaxonCounts\(taxonId\)/);
  assert.match(taxonTree, /`\$\{counts\.taxon_count\} taxa, \$\{counts\.photo_count\} photos`/);
  const countReporting = taxonTree.slice(taxonTree.indexOf("const reportTaxonCounts"), taxonTree.indexOf("function enterTaxon"));
  assert.doesNotMatch(countReporting, /rows\.length/);
});

test("map reports loaded viewport photos through status without an overlay", () => {
  const photos = source("../src/features/photos/PhotosView.tsx");
  const map = photos.slice(photos.indexOf("export function PhotoMapView"));
  assert.match(map, /onStatus: \(message: string\) => void;/);
  assert.match(map, /onStatus\(`\$\{page\.items\.length\} photos in view`\)/);
  assert.doesNotMatch(map, /map-count/);
});

test("taxonomy search reports loading and shown-result status", () => {
  const taxonomy = source("../src/features/taxonomy/TaxonomyView.tsx");
  const search = taxonomy.slice(taxonomy.indexOf("export function TaxonomySearchView"), taxonomy.indexOf("const inputFields"));
  assert.match(search, /onStatus: \(message: string\) => void;/);
  assert.match(search, /onStatus\("Searching\.\.\."\)/);
  assert.match(search, /"No results"/);
  assert.match(search, /`\$\{taxonomySearch\.results\.length\} results shown`/);
});
