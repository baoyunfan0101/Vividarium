import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { photoPaneHeaderLines } from "../src/features/photos/photoFormatting.ts";

test("uses filename and file metadata for every photo pane header", () => {
  const lines = photoPaneHeaderLines({
    filename: "Panthera_leo_002.jpg",
    file_size: 13_002_342,
    modified_at_ns: Date.UTC(2026, 7, 22, 14, 30) * 1_000_000,
  });

  assert.equal(lines.filename, "Panthera_leo_002.jpg");
  assert.match(lines.summary, /^12\.4 MB \u00b7 /);
  assert.doesNotMatch(lines.summary, /Panthera|Felidae|Taxon/);
});

test("uses the shared header and layout in every main photo pane", () => {
  for (const path of [
    "../src/features/photos/PhotoBrowser.tsx",
    "../src/features/photos/PhotoDetailView.tsx",
    "../src/features/mapping/MappingEditor.tsx",
    "../src/features/mapping/MappingView.tsx",
  ]) {
    const source = readFileSync(new URL(path, import.meta.url), "utf8");
    assert.match(source, /PhotoPaneHeader/);
    assert.match(source, /photo-pane-heading/);
  }
});

test("uses the shared 42px height for every main photo pane header", () => {
  const theme = readFileSync(new URL("../src/styles/theme.css", import.meta.url), "utf8");
  const photoStyles = readFileSync(new URL("../src/styles/photos.css", import.meta.url), "utf8");
  const mappingStyles = readFileSync(new URL("../src/styles/mapping.css", import.meta.url), "utf8");

  assert.match(theme, /--photo-pane-header-height: 42px/);
  assert.match(photoStyles, /\.photo-browser-list, \.photo-browser-main.*grid-template-rows: var\(--photo-pane-header-height\)/);
  assert.match(photoStyles, /\.photo-detail-view.*grid-template-rows: var\(--photo-pane-header-height\)/);
  assert.match(mappingStyles, /\.mapping-photo-stage\.with-header.*grid-template-rows: var\(--photo-pane-header-height\)/);
  assert.match(mappingStyles, /\.editor-photo-column.*grid-template-rows: var\(--photo-pane-header-height\)/);
});
