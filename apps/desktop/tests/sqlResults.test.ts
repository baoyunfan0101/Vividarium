import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import type { CustomSqlExecutionResult, SqlResultSet } from "../src/api/customSql.ts";
import {
  formatAffectedRows,
  formatRowCount,
  formatSqlExecutionStatus,
  maxSqlResultColumnCount,
  sqlResultSetHeight,
  sqlResultTableMinWidth,
  sqlStatementOutputs,
} from "../src/features/taxonomy/sqlResults.ts";

function source(path: string) {
  return readFileSync(new URL(path, import.meta.url), "utf8");
}

function resultSet(statementIndex: number, columnCount: number): SqlResultSet {
  return {
    statement_index: statementIndex,
    columns: Array.from({ length: columnCount }, (_, index) => ({
      name: `column_${index + 1}`,
      declared_type: null,
    })),
    rows: [],
    truncated: false,
  };
}

test("uses the largest result-set column count for execution width", () => {
  const resultSets = [resultSet(1, 2), resultSet(2, 5), resultSet(3, 3)];
  assert.equal(maxSqlResultColumnCount(resultSets), 5);
  assert.equal(sqlResultTableMinWidth(maxSqlResultColumnCount(resultSets)), 650);
  assert.equal(maxSqlResultColumnCount([]), 1);
});

test("sizes SQL result sets to their own preview row count up to the CSS cap", () => {
  assert.equal(sqlResultSetHeight(0), 68);
  assert.equal(sqlResultSetHeight(1), 100);
  assert.equal(sqlResultSetHeight(3), 164);
  assert.equal(sqlResultSetHeight(7), 292);
  assert.equal(sqlResultSetHeight(100), 3268);

  const view = source("../src/features/taxonomy/CustomSqlView.tsx");
  const styles = source("../src/styles/taxonomy.css");
  assert.match(view, /const resultHeight = sqlResultSetHeight\(result\.rows\.length\);/);
  assert.match(view, /style=\{\{ height: `min\(\$\{resultHeight\}px, 320px, 42vh\)` \}\}/);
  assert.doesNotMatch(styles, /\.sql-result-set \{[^}]*height:/);
});

test("associates result sets and mutation messages by statement index", () => {
  const result: CustomSqlExecutionResult = {
    operation_id: 1,
    changeset_size: 12,
    result_sets: [resultSet(1, 2), resultSet(3, 1)],
    messages: [
      { statement_index: 1, affected_rows: null, message: "query" },
      { statement_index: 2, affected_rows: 4, message: "mutation" },
      { statement_index: 3, affected_rows: 1, message: "mutation returning" },
    ],
    script_saved: true,
    warnings: [],
  };

  assert.deepEqual(
    sqlStatementOutputs(result).map((output) => ({
      statementIndex: output.statementIndex,
      hasResultSet: output.resultSet !== null,
      affectedRows: output.affectedRows,
      exportAllowed: output.exportAllowed,
    })),
    [
      { statementIndex: 1, hasResultSet: true, affectedRows: null, exportAllowed: true },
      { statementIndex: 2, hasResultSet: false, affectedRows: 4, exportAllowed: false },
      { statementIndex: 3, hasResultSet: true, affectedRows: 1, exportAllowed: false },
    ],
  );
});

test("formats compact execution status for the tab status bar", () => {
  const base: CustomSqlExecutionResult = {
    operation_id: null,
    changeset_size: 0,
    result_sets: [],
    messages: [],
    script_saved: true,
    warnings: [],
  };
  assert.equal(
    formatSqlExecutionStatus(base),
    "0 bytes changed · No operation created · Script saved",
  );
  assert.equal(
    formatSqlExecutionStatus({ ...base, changeset_size: 384, operation_id: 41 }),
    "384 bytes changed · Operation 41 · Script saved",
  );
  assert.equal(
    formatSqlExecutionStatus({ ...base, changeset_size: 384, operation_id: 41, script_saved: false, warnings: ["warning text"] }),
    "384 bytes changed · Operation 41 · Script not saved · warning text",
  );
  assert.equal(formatRowCount(1), "1 row");
  assert.equal(formatAffectedRows(2), "2 rows affected");
});

test("Custom SQL retains the successful execution snapshot for output and export", () => {
  const view = source("../src/features/taxonomy/CustomSqlView.tsx");
  const api = source("../src/api/customSql.ts");
  assert.match(view, /const executedSql = sql;/);
  assert.match(view, /setExecution\(\{ sql: executedSql, result: next \}\)/);
  assert.match(view, /onStatus\(formatSqlExecutionStatus\(next\)\)/);
  assert.doesNotMatch(view, /className="sql-result-summary"/);
  assert.match(view, /exportCustomSqlQuery\(\s*execution\.sql,\s*statementIndex,/);
  assert.doesNotMatch(view, /setExecution\(null\)/);
  assert.match(api, /request: \{ sql, statement_index: statementIndex, destination_path: destinationPath \}/);
});

test("SQL result rows share table width and clip long cells", () => {
  const view = source("../src/features/taxonomy/CustomSqlView.tsx");
  const styles = source("../src/styles/taxonomy.css");
  assert.match(view, /minWidth: sqlResultTableMinWidth\(executionColumnCount\)/);
  assert.match(view, /gridTemplateColumns: template/);
  assert.doesNotMatch(styles, /\.sql-result-row[^}]*min-width: max-content/);
  assert.match(styles, /\.sql-result-header, \.sql-result-row \{[^}]*width: 100%;[^}]*min-width: 0;/);
  assert.match(styles, /\.sql-result-header span, \.sql-result-row code \{[^}]*overflow: hidden;[^}]*text-overflow: ellipsis;[^}]*white-space: nowrap;/);
});

test("SQL output scrolls between statements and combines RETURNING output", () => {
  const view = source("../src/features/taxonomy/CustomSqlView.tsx");
  const styles = source("../src/styles/taxonomy.css");
  assert.match(styles, /\.sql-results \{[^}]*width: 100%;[^}]*height: 100%;[^}]*min-height: 0;[^}]*overflow: auto;/);
  assert.match(view, /statement\.resultSet \? \(/);
  assert.match(view, /\) : statement\.affectedRows !== null \? \(/);
  assert.match(view, /affectedRows=\{statement\.affectedRows\}/);
  assert.match(view, /formatAffectedRows\(affectedRows\)/);
});
