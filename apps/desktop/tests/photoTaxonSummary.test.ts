import assert from "node:assert/strict";
import test from "node:test";
import type { TaxonDisplaySummary } from "../src/api/taxonomy.ts";
import {
  loadSelectedPhotoTaxonSummary,
  PhotoTaxonSummaryCache,
} from "../src/features/photos/photoTaxonSummaryCache.ts";
import {
  loadSelectedPhotoMappingStatus,
  PhotoMappingStatusCache,
} from "../src/features/photos/photoMappingStatusCache.ts";
import {
  statusBarMappingStatus,
  type PhotoTaxonDisplayState,
} from "../src/features/photos/photoTaxonDisplayState.ts";
import {
  formatTaxonDisplaySummary,
  taxonDisplayShrinkWeight,
} from "../src/features/taxonomy/taxonNameFormatting.ts";

const summary = (name: string): TaxonDisplaySummary => ({
  current_rank: "species",
  items: [{
    taxon_id: 1,
    rank: "species",
    names: { sci_name: name, zh_name: null, en_name: null },
  }],
});

test("does not query without a selected photo and caches repeated selection", async () => {
  let calls = 0;
  const cache = new PhotoTaxonSummaryCache(async (photoId) => {
    calls += 1;
    return summary(`Photo ${photoId}`);
  });

  assert.equal(await loadSelectedPhotoTaxonSummary(null, cache), null);
  assert.equal(calls, 0);
  assert.deepEqual(await loadSelectedPhotoTaxonSummary(7, cache), summary("Photo 7"));
  assert.deepEqual(await loadSelectedPhotoTaxonSummary(7, cache), summary("Photo 7"));
  assert.deepEqual(await loadSelectedPhotoTaxonSummary(8, cache), summary("Photo 8"));
  assert.equal(calls, 2);
});

test("narrow display shrinks coarse ranks before the current taxon", () => {
  assert.deepEqual(
    [0, 1, 2].map((index) => taxonDisplayShrinkWeight(index, 3)),
    [1000, 100, 1],
  );
});

test("mapping invalidation reloads only the stale photo summary", async () => {
  let version = 1;
  let calls = 0;
  const cache = new PhotoTaxonSummaryCache(async (photoId) => {
    calls += 1;
    return summary(`Photo ${photoId} v${version}`);
  });
  await cache.load(7);
  await cache.load(8);
  version = 2;
  cache.invalidate(7);

  assert.deepEqual(await cache.load(7), summary("Photo 7 v2"));
  assert.deepEqual(await cache.load(8), summary("Photo 8 v1"));
  assert.equal(calls, 3);
});

test("a stale mapping response cannot repopulate the invalidated status cache", async () => {
  const requests: Array<(status: "matched" | "unmatched") => void> = [];
  const cache = new PhotoMappingStatusCache(() => new Promise((resolve) => requests.push(resolve)));

  const first = cache.load(7);
  cache.invalidate(7);
  const second = cache.load(7);
  requests[1]("unmatched");
  assert.equal(await second, "unmatched");
  requests[0]("matched");
  assert.equal(await first, "matched");

  assert.equal(await loadSelectedPhotoMappingStatus(7, cache), "unmatched");
  assert.equal(requests.length, 2);
});

test("formats a photo display path with the selected accepted names", () => {
  const path: TaxonDisplaySummary = {
    current_rank: "species",
    items: [
      { taxon_id: 1, rank: "family", names: { sci_name: "Felidae", zh_name: "Cat family", en_name: null } },
      { taxon_id: 2, rank: "genus", names: { sci_name: "Panthera", zh_name: "Panther genus", en_name: null } },
      { taxon_id: 3, rank: "species", names: { sci_name: "Panthera leo", zh_name: "Lion Chinese", en_name: "Lion" } },
    ],
  };
  assert.equal(
    formatTaxonDisplaySummary(path, { sci_name: true, zh_name: true, en_name: false }),
    "Felidae \u00b7 Cat family > Panthera \u00b7 Panther genus > Panthera leo \u00b7 Lion Chinese",
  );
});

test("uses the current photo mapping status when no taxon path is available", () => {
  const state = (mappingStatus: PhotoTaxonDisplayState["mappingStatus"]): PhotoTaxonDisplayState => ({
    summary: null,
    mappingStatus,
  });
  assert.equal(statusBarMappingStatus({ summary: summary("Felis catus"), mappingStatus: "matched" }), null);
  assert.equal(statusBarMappingStatus(state("matched")), "matched");
  assert.equal(statusBarMappingStatus(state("ambiguous")), "ambiguous");
  assert.equal(statusBarMappingStatus(state("unmatched")), "unmatched");
  assert.equal(statusBarMappingStatus(state("processing")), "processing");
  assert.equal(statusBarMappingStatus(null), null);
});
