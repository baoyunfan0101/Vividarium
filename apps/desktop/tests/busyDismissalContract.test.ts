import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

function source(path: string) {
  return readFileSync(new URL(path, import.meta.url), "utf8");
}

test("Modal owns default and locked dismissal behavior", () => {
  const ui = source("../src/shared/ui.tsx");
  assert.match(ui, /dismissible = true/);
  assert.match(ui, /if \(event\.key === "Escape" && dismissible\) onClose\(\);/);
  assert.match(ui, /onMouseDown=\{\(\) => dismissible && onClose\(\)\}/);
  assert.match(ui, /<IconButton onClick=\{onClose\} disabled=\{!dismissible\} aria-label="Close">/);
});

test("photo and folder rename modals lock dismissal while running", () => {
  const photo = source("../src/features/photos/PhotoContextMenu.tsx");
  const directory = source("../src/features/photos/DirectoryContextMenu.tsx");
  for (const menu of [photo, directory]) {
    assert.match(menu, /if \(renaming \|\| busy\) return;/);
    assert.match(menu, /\}, \[busy, onClose, renaming\]\);/);
    assert.match(menu, /dismissible=\{!busy\}/);
    assert.match(menu, /<Button disabled=\{Boolean\(busy\)\} onClick=\{\(\) => setRenaming\(false\)\}>Cancel<\/Button>/);
    assert.match(menu, /await action\(\);\s*onClose\(\);/);
  }
  assert.match(photo, /label="Rename" disabled=\{Boolean\(busy\)\}/);
  assert.match(photo, /busy === "Renaming" \? "Renaming\.\.\." : "Rename"/);
  assert.match(directory, /busy === "Renaming folder" \? "Renaming\.\.\." : "Rename"/);
});

test("busy taxonomy dialogs use shared modal dismissal state", () => {
  const sqlImport = source("../src/features/taxonomy/SqlImportSettings.tsx");
  const confirmation = source("../src/features/taxonomy/TaxonConfirmationModal.tsx");
  const inputs = source("../src/features/taxonomy/SqlInputList.tsx");
  assert.match(sqlImport, /dismissible=\{!busy\}\s*onClose=\{\(\) => setConfirming\(false\)\}/);
  assert.match(confirmation, /dismissible=\{!busy\}\s*onClose=\{onClose\}/);
  assert.match(inputs, /dismissible=\{!adding\}\s*onClose=\{\(\) => setPending\(null\)\}/);
  assert.doesNotMatch(sqlImport, /onClose=\{\(\) => !busy/);
  assert.doesNotMatch(confirmation, /onClose=\{\(\) => !busy/);
  assert.doesNotMatch(inputs, /if \(!adding\) setPending/);
});
