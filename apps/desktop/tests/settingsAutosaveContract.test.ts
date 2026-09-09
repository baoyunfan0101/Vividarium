import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { renameFromTaxonomyStatus } from "../src/features/photos/photoRenameStatus.ts";
import { formatPhotoRenameSummary } from "../src/features/photos/photoOperation.ts";
import {
  canPresentHookResult,
  hookDraftMatchesSnapshot,
  replaceTestedHookSnapshot,
} from "../src/features/settings/hookAsyncState.ts";

function source(path: string) {
  return readFileSync(new URL(path, import.meta.url), "utf8");
}

test("ordinary Settings sections autosave through the tab status bar", () => {
  const settings = source("../src/features/settings/SettingsView.tsx");
  const shell = source("../src/app/DesktopShell.tsx");
  assert.match(settings, /onStatus: \(message: string\) => void;/);
  assert.match(shell, /<SettingsView[^;]+onStatus=\{onStatus\}/);
  assert.doesNotMatch(settings, /startPhotoMapping/);
  assert.match(settings, /onStatus\("Settings saved\."\)/);
  assert.match(settings, /onStatus\("Naming settings saved\."\)/);
  assert.match(settings, /onStatus\("Map settings saved\."\)/);
  assert.match(settings, /if \(sequence === saveSequence\.current\) \{\s*onChange\(previous\);\s*setSaveError\(errorMessage\(nextError\)\);\s*onStatus\("Settings save failed\."\);/);
  assert.match(settings, /if \(sequence === saveSequence\.current\) \{\s*setPriority\(previous\);\s*setSaveError\(errorMessage\(nextError\)\);\s*onStatus\("Naming settings save failed\."\);/);
  assert.equal((settings.match(/onStatus\("Naming settings save failed\."\)/g) ?? []).length, 3);
  assert.match(settings, /if \(sequence === saveSequence\.current\) \{\s*setSettings\(previous\);\s*setTokenDraft\(previous\.tianditu_token \?\? ""\);\s*setError\(errorMessage\(nextError\)\);\s*onStatus\("Map settings save failed\."\);/);
  assert.match(settings, /onStatus\("Hook operation failed\."\)/);
  assert.doesNotMatch(settings, /Naming settings saved\.<\/div>/);
  assert.doesNotMatch(settings, /Map metadata saved/);
});

test("Hook results present only for the active unchanged Hook draft", () => {
  assert.equal(canPresentHookResult("photo_filename", "photo_filename", 1, 1), true);
  assert.equal(canPresentHookResult("photo_filename", "photo_filename", 1, 2), false);
  assert.equal(canPresentHookResult("photo_filename", "synonym_authority", 1, 1), false);
  assert.equal(canPresentHookResult("photo_filename", "photo_filename", 1, 1), true);
  const settings = source("../src/features/settings/SettingsView.tsx");
  assert.match(settings, /const activeKindRef = useRef\(kind\);/);
  assert.match(settings, /activeKindRef\.current = kind;/);
  assert.match(settings, /if \(activeKindRef\.current === testedKind\) setError\(errorMessage\(nextError\)\);/);
});

test("text and numeric preferences save only when editing completes", () => {
  const settings = source("../src/features/settings/SettingsView.tsx");
  assert.match(settings, /value=\{recentDraft\}[\s\S]*onChange=\{\(event\) => setRecentDraft\(event\.target\.value\)\}[\s\S]*onBlur=\{saveRecentSearchLimit\}/);
  assert.match(settings, /value=\{separator\}[\s\S]*onBlur=\{\(\) => void saveSeparator\(\)\}/);
  assert.match(settings, /value=\{tokenDraft\}[\s\S]*onBlur=\{saveToken\}/);
});

test("Hook tests authorize an explicit save for the exact tested snapshot", () => {
  const settings = source("../src/features/settings/SettingsView.tsx");
  const runStart = settings.indexOf("async function run()");
  const saveStart = settings.indexOf("async function save()");
  assert.ok(runStart >= 0);
  assert.ok(saveStart > runStart);
  const run = settings.slice(runStart, saveStart);
  assert.match(settings, /const snapshot = \{\s*script: scripts\[testedKind\],\s*cases: cases\[testedKind\],\s*\};/);
  assert.match(settings, /runNamingHookTests\(testedKind, snapshot\.script, snapshot\.cases\)/);
  assert.doesNotMatch(run, /saveNamingHook/);
  assert.match(settings, /const \[testedSnapshots, setTestedSnapshots\]/);
  assert.match(settings, /setTestedSnapshots\(\(current\) => replaceTestedHookSnapshot\(current, testedKind, null\)\);\s*setBusy\("Testing Hook"\)/);
  assert.match(settings, /if \(next\.failed === 0\) \{[\s\S]*replaceTestedHookSnapshot\(current, testedKind, snapshot\)/);
  assert.match(settings, /setTestedSnapshots\(\(current\) => replaceTestedHookSnapshot\(current, kind, null\)\)/);
  assert.match(settings, /const saveAvailable = hookDraftMatchesSnapshot/);
  assert.match(settings, /async function save\(\)[\s\S]*saveNamingHook\(testedKind, snapshot\.script, snapshot\.cases\)/);
  assert.match(settings, /<Button variant="primary" disabled=\{Boolean\(busy\) \|\| !saveAvailable\} onClick=\{\(\) => void save\(\)\}><Save size=\{13\} \/>\{busy === "Saving Hook" \? "Saving\.\.\." : "Save"\}<\/Button>/);
});

test("Hook Save eligibility requires the exact tested draft", () => {
  const snapshot = {
    script: "fn hook(value) { value }",
    cases: [{ input: "input", expected: { kind: "photo_filename" as const, output: {} } }],
  };
  assert.equal(hookDraftMatchesSnapshot(snapshot.script, snapshot.cases, snapshot), true);
  assert.equal(hookDraftMatchesSnapshot("fn hook(value) { value + 1 }", snapshot.cases, snapshot), false);
  assert.equal(
    hookDraftMatchesSnapshot(snapshot.script, [{ ...snapshot.cases[0], input: "changed" }], snapshot),
    false,
  );
  assert.equal(
    hookDraftMatchesSnapshot(
      snapshot.script,
      [{ ...snapshot.cases[0], expected: { kind: "photo_filename", output: { sci_name: "Changed" } } }],
      snapshot,
    ),
    false,
  );
});

test("re-testing revokes the previous Hook Save authorization", () => {
  const snapshot = {
    script: "fn hook(value) { value }",
    cases: [{ input: "input", expected: { kind: "photo_filename" as const, output: {} } }],
  };
  const empty = { photo_filename: null, synonym_authority: null };
  const firstPass = replaceTestedHookSnapshot(empty, "photo_filename", snapshot);
  assert.equal(hookDraftMatchesSnapshot(snapshot.script, snapshot.cases, firstPass.photo_filename), true);

  const retest = replaceTestedHookSnapshot(firstPass, "photo_filename", null);
  assert.equal(hookDraftMatchesSnapshot(snapshot.script, snapshot.cases, retest.photo_filename), false);
  assert.equal(retest.synonym_authority, null);

  const failedRetest = retest;
  assert.equal(hookDraftMatchesSnapshot(snapshot.script, snapshot.cases, failedRetest.photo_filename), false);

  const erroredRetest = retest;
  assert.equal(hookDraftMatchesSnapshot(snapshot.script, snapshot.cases, erroredRetest.photo_filename), false);

  const successfulRetest = replaceTestedHookSnapshot(retest, "photo_filename", snapshot);
  assert.equal(hookDraftMatchesSnapshot(snapshot.script, snapshot.cases, successfulRetest.photo_filename), true);
});

test("Rename from taxonomy reports changed and unchanged outcomes", () => {
  assert.equal(renameFromTaxonomyStatus("old.jpg", "Canis lupus.jpg"), "Renamed to Canis lupus.jpg");
  assert.equal(
    renameFromTaxonomyStatus("Canis lupus.jpg", "Canis lupus.jpg"),
    "Filename already matches the naming settings.",
  );
  const menu = source("../src/features/photos/PhotoContextMenu.tsx");
  assert.match(menu, /onStatus\(renameFromTaxonomyStatus\(photo\.filename, renamed\.filename\)\)/);
  assert.equal(
    formatPhotoRenameSummary({ operation_id: 7, total: 23, applied: 18, no_change: 4, failed: 1 }),
    "Renamed 18 photos, 4 unchanged, 1 failed",
  );
});
