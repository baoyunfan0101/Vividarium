import assert from "node:assert/strict";
import test from "node:test";
import {
  SQL_IMPORT_VALIDATION_ISSUE_ROW_HEIGHT,
  sqlImportValidationIssueRow,
} from "../src/features/taxonomy/sqlImportValidation.ts";
import { clampVirtualScrollTop } from "../src/shared/virtualScroll.ts";

test("keeps long SQL import validation issues at a fixed compact height", () => {
  const message = `Taxon 358 has duplicate names after canonical normalization: ${"Zootoca vivipara ".repeat(20)}`;
  const row = sqlImportValidationIssueRow({
    code: "duplicate_canonical_name",
    message,
    taxon_id: 358,
    related_taxon_id: null,
    table: "taxon_names",
    row_identifier: "791",
  });

  assert.equal(SQL_IMPORT_VALIDATION_ISSUE_ROW_HEIGHT, 44);
  assert.equal(row.message, message);
  assert.equal(row.context, "Taxon 358");
});

test("keeps SQL import issue context when taxon IDs are unavailable", () => {
  const row = sqlImportValidationIssueRow({
    code: "invalid_name",
    message: "Invalid name",
    taxon_id: null,
    related_taxon_id: null,
    table: "taxon_names",
    row_identifier: "42",
  });

  assert.equal(row.context, "taxon_names / 42");
});

test("clamps large SQL import issue lists using the compact row height", () => {
  const issueCount = 10_000;
  const viewportHeight = SQL_IMPORT_VALIDATION_ISSUE_ROW_HEIGHT * 5;
  const maximumScrollTop = issueCount * SQL_IMPORT_VALIDATION_ISSUE_ROW_HEIGHT - viewportHeight;

  assert.equal(
    clampVirtualScrollTop(
      maximumScrollTop + 10_000,
      issueCount,
      SQL_IMPORT_VALIDATION_ISSUE_ROW_HEIGHT,
      viewportHeight,
    ),
    maximumScrollTop,
  );
});
