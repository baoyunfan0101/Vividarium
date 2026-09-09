import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(
  new URL("../src/features/photos/PhotosView.tsx", import.meta.url),
  "utf8",
);
const display = readFileSync(
  new URL("../src/features/photos/PhotoDisplay.tsx", import.meta.url),
  "utf8",
);

test("folder and taxon breadcrumbs clear tree row and photo selection", () => {
  const folderSection = source.slice(source.indexOf("export function FolderPhotosView"), source.indexOf("export function TaxonPhotosView"));
  const taxonSection = source.slice(source.indexOf("export function TaxonPhotosView"));
  for (const section of [folderSection, taxonSection]) {
    assert.equal((section.match(/setTrail\(\[\]\)/g) ?? []).length, 1);
    assert.equal((section.match(/setTrail\(trail\.slice/g) ?? []).length, 1);
    assert.equal((section.match(/setActiveRowKey\(null\);/g) ?? []).length >= 3, true);
    assert.equal((section.match(/interaction\.clearSelection\(\);/g) ?? []).length >= 3, true);
  }
});

test("folder and taxon lists use explicit keyboard entry without thumbnail autofocus", () => {
  const folderSection = source.slice(source.indexOf("export function FolderPhotosView"), source.indexOf("export function TaxonPhotosView"));
  const taxonSection = source.slice(source.indexOf("export function TaxonPhotosView"));
  for (const section of [folderSection, taxonSection]) {
    assert.match(section, /const listRef = useRef<VirtualListHandle>\(null\);/);
    assert.match(section, /const openFullscreen = useCallback\(\(photo: Photo\) => \{\s*handlers\.openFullscreen\(photo, \(\) => listRef\.current\?\.focus\(\)\);/);
    assert.match(section, /handlers: viewHandlers,/);
    assert.match(section, /onOpenFullscreen: openFullscreen/);
    assert.match(section, /usePhotoTreeListEntry\(\{/);
    assert.match(section, /<VirtualList\s+ref=\{listRef\}/);
    assert.doesNotMatch(section, /focusWhen=/);
  }
  assert.match(source, /listRef\.current\?\.focus\(\);/);
  assert.match(source, /selectedPhotoId,/);
  assert.match(source, /resolvePhotoListEntryIndex/);
  assert.equal((source.match(/onEscapeToThumbnails: \(\) => listRef\.current\?\.focus\(\)/g) ?? []).length, 2);
  assert.match(display, /onEscapeToThumbnailsRef\.current\?\.\(\);/);
});

test("tree keyboard entry yields to open overlays while retaining breadcrumb entry", () => {
  assert.match(source, /function hasBlockingTreeEntryOverlay\(\): boolean/);
  assert.match(source, /document\.querySelector\("\.context-menu, \[role='dialog'\], \.modal-card"\)/);
  assert.match(source, /if \(hasBlockingTreeEntryOverlay\(\)\) return;/);
  assert.match(source, /if \(isPhotoFullscreenActive\(\)\) return;/);
  assert.match(source, /!target\.closest\("\.breadcrumbs"\)/);
});
