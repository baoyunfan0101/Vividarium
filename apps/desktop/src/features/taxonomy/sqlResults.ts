import type { CustomSqlExecutionResult, SqlResultSet } from "../../api/customSql";

export const SQL_RESULT_COLUMN_MIN_WIDTH = 130;
export const SQL_RESULT_SET_CHROME_HEIGHT = 68;
export const SQL_RESULT_ROW_HEIGHT = 32;

export type CustomSqlExecutionSnapshot = {
  sql: string;
  result: CustomSqlExecutionResult;
};

export type SqlStatementOutput = {
  statementIndex: number;
  resultSet: SqlResultSet | null;
  affectedRows: number | null;
  exportAllowed: boolean;
};

export function maxSqlResultColumnCount(resultSets: SqlResultSet[]): number {
  return Math.max(1, ...resultSets.map((resultSet) => resultSet.columns.length));
}

export function sqlResultTableMinWidth(executionColumnCount: number): number {
  return Math.max(1, executionColumnCount) * SQL_RESULT_COLUMN_MIN_WIDTH;
}

export function sqlResultSetHeight(rowCount: number): number {
  return SQL_RESULT_SET_CHROME_HEIGHT + rowCount * SQL_RESULT_ROW_HEIGHT;
}

export function formatSqlExecutionStatus(result: CustomSqlExecutionResult): string {
  const operation = result.operation_id === null
    ? "No operation created"
    : `Operation ${result.operation_id}`;
  const saved = result.script_saved ? "Script saved" : "Script not saved";
  return [
    `${result.changeset_size} bytes changed`,
    operation,
    saved,
    ...result.warnings,
  ].join(" · ");
}

export function formatRowCount(count: number): string {
  return `${count} ${count === 1 ? "row" : "rows"}`;
}

export function formatAffectedRows(count: number): string {
  return `${formatRowCount(count)} affected`;
}

export function sqlStatementOutputs(result: CustomSqlExecutionResult): SqlStatementOutput[] {
  const indexes = new Set([
    ...result.messages.map((message) => message.statement_index),
    ...result.result_sets.map((resultSet) => resultSet.statement_index),
  ]);
  return [...indexes].sort((left, right) => left - right).map((statementIndex) => {
    const resultSet = result.result_sets.find(
      (candidate) => candidate.statement_index === statementIndex,
    ) ?? null;
    const message = result.messages.find(
      (candidate) => candidate.statement_index === statementIndex,
    );
    return {
      statementIndex,
      resultSet,
      affectedRows: message?.affected_rows ?? null,
      exportAllowed: resultSet !== null && message?.affected_rows === null,
    };
  });
}
