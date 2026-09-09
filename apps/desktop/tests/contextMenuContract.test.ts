import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

function source(path: string) {
  return readFileSync(new URL(path, import.meta.url), "utf8");
}

test("the application suppresses native context menus without blocking bubbling", () => {
  const app = source("../src/App.tsx");
  const shell = source("../src/app/DesktopShell.tsx");
  assert.match(app, /document\.addEventListener\("contextmenu", preventNativeContextMenu\)/);
  assert.match(app, /document\.removeEventListener\("contextmenu", preventNativeContextMenu\)/);
  assert.match(app, /event\.preventDefault\(\)/);
  assert.doesNotMatch(app, /stop(?:Immediate)?Propagation/);
  assert.doesNotMatch(shell, /desktop-shell" onContextMenu/);
});

test("standalone mapping editor owns the shared photo context menu", () => {
  const editor = source("../src/features/mapping/MappingEditor.tsx");
  assert.match(editor, /function StandaloneMappingPhotoPane/);
  assert.match(editor, /usePhotoInteraction/);
  assert.match(editor, /<PhotoStage photo=\{photo\} onContextMenu=\{interaction\.openContextMenu\}/);
  assert.match(editor, /\{interaction\.contextMenu\}/);
  assert.match(editor, /if \(embedded\) return/);
});

test("every interactive full-photo view supplies the shared context menu handler", () => {
  for (const path of [
    "../src/features/photos/PhotoBrowser.tsx",
    "../src/features/photos/PhotosView.tsx",
    "../src/features/photos/PhotoDetailView.tsx",
    "../src/features/mapping/MappingView.tsx",
  ]) {
    assert.match(source(path), /onContextMenu=\{interaction\.openContextMenu\}/);
  }
});

test("photo context menu uses the current mapping status in its taxon action", () => {
  const menu = source("../src/features/photos/PhotoContextMenu.tsx");
  assert.doesNotMatch(menu, /Mapping state/);
  assert.match(menu, /label="View photo details"/);
  assert.match(menu, /label="View fullscreen"/);
  assert.match(menu, /label="View taxon details"/);
  assert.match(menu, /trailing=\{mapping \? <MappingBadge status=\{mapping\.status\}/);
  assert.match(menu, /const matched = mapping\?\.status === "matched" && mapping\.taxon_id !== null;/);
  assert.match(menu, /label="View taxon details"[\s\S]*disabled=\{!matched\}/);

  const labels = [
    "View fullscreen",
    "View photo details",
    "View taxon details",
    "Edit mapping",
    "Remap from filename",
    "Rename",
    "Rename from taxonomy",
    "Reveal in Finder / Explorer",
  ];
  const positions = labels.map((label) => menu.indexOf(`label="${label}"`));
  assert.ok(positions.every((position) => position >= 0));
  assert.deepEqual([...positions].sort((left, right) => left - right), positions);

  const [fullscreen, details, taxon, edit, remap, rename, renameFromTaxonomy, reveal] = positions;
  const firstSeparator = menu.indexOf("<MenuSeparator />", fullscreen);
  const secondSeparator = menu.indexOf("<MenuSeparator />", remap);
  const thirdSeparator = menu.indexOf("<MenuSeparator />", renameFromTaxonomy);
  assert.ok(fullscreen < details && details < firstSeparator && firstSeparator < taxon);
  assert.ok(remap < secondSeparator && secondSeparator < rename);
  assert.ok(renameFromTaxonomy < thirdSeparator && thirdSeparator < reveal);
});

test("photo context fullscreen uses the context-menu target photo", () => {
  const interaction = source("../src/features/photos/PhotoInteraction.tsx");
  assert.match(interaction, /onOpenFullscreen=\{\(\) => handlers\.openFullscreen\(context\.photo\)\}/);
});

test("list-backed context menus retain their owning list focus callback", () => {
  for (const path of [
    "../src/features/photos/PhotoBrowser.tsx",
    "../src/features/photos/PhotosView.tsx",
  ]) {
    const view = source(path);
    assert.match(view, /handlers\.openFullscreen\(photo, \(\) => listRef\.current\?\.focus\(\)\)/);
    assert.match(view, /handlers: viewHandlers/);
  }
});
