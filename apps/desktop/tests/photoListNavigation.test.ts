import assert from "node:assert/strict";
import test from "node:test";
import {
  resolvePhotoListEntryIndex,
  treeArrowAction,
} from "../src/features/photos/photoListNavigation.ts";

test("right expands only a collapsed tree item", () => {
  assert.equal(treeArrowAction(false, 1), "expand");
  assert.equal(treeArrowAction(true, 1), null);
});

test("left collapses only an expanded tree item", () => {
  assert.equal(treeArrowAction(true, -1), "collapse");
  assert.equal(treeArrowAction(false, -1), null);
});

test("arrow entry restores the selected photo row without moving it", () => {
  const rows = ["folder", "photo-12", "photo-13"];
  const options = {
    rows,
    selectedPhotoId: 12,
    getPhotoId: (row) => row.startsWith("photo-") ? Number(row.slice(6)) : null,
  };
  assert.equal(resolvePhotoListEntryIndex({ ...options, direction: 1 }), 1);
  assert.equal(resolvePhotoListEntryIndex({ ...options, direction: -1 }), 1);
});

test("arrow entry selects the first or last photo while skipping tree rows", () => {
  const rows = ["folder", "photo-12", "load-more", "photo-13"];
  const getPhotoId = (row: string) => row.startsWith("photo-") ? Number(row.slice(6)) : null;
  assert.equal(resolvePhotoListEntryIndex({ rows, selectedPhotoId: null, direction: 1, getPhotoId }), 1);
  assert.equal(resolvePhotoListEntryIndex({ rows, selectedPhotoId: null, direction: -1, getPhotoId }), 3);
  assert.equal(resolvePhotoListEntryIndex({ rows: ["folder", "load-more"], selectedPhotoId: null, direction: 1, getPhotoId }), -1);
});
