import { call } from "./client";
import { demoCompletedOperation, type OperationState } from "./tasks";

export type SqlValue =
  | { type: "null" }
  | { type: "integer"; value: number }
  | { type: "real"; value: number }
  | { type: "text"; value: string }
  | { type: "blob"; value: string };
export type SqlColumn = { name: string; declared_type: string | null };
export type SqlResultSet = {
  statement_index: number;
  columns: SqlColumn[];
  rows: SqlValue[][];
  truncated: boolean;
};
export type SqlStatementMessage = {
  statement_index: number;
  affected_rows: number | null;
  message: string;
};
export type CustomSqlExecutionResult = {
  operation_id: number | null;
  changeset_size: number;
  result_sets: SqlResultSet[];
  messages: SqlStatementMessage[];
  script_saved: boolean;
  warnings: string[];
};
export type SqlExportResult = { path: string; row_count: number };
export type SqlSourceObject = {
  name: string;
  object_type: "table" | "view" | "virtual_table";
  columns: SqlColumn[];
};
export type SqlSourceSchema = { alias: string; objects: SqlSourceObject[] };
export type PersistentSqlInput = {
  kind: "sqlite" | "csv";
  alias: string;
  original_path: string;
  available: boolean;
  schema: SqlSourceSchema;
};
export type AddSqlInputResult = {
  input: PersistentSqlInput;
  inputs: PersistentSqlInput[];
  warnings: string[];
};
export type RemoveSqlInputResult = { inputs: PersistentSqlInput[]; warnings: string[] };

export function demoSqlInput(kind: "csv" | "sqlite", alias: string, path: string): PersistentSqlInput {
  return {
    kind,
    alias,
    original_path: path,
    available: true,
    schema: {
      alias,
      objects: [{
        name: kind === "csv" ? alias : "taxa",
        object_type: "table",
        columns: [{ name: "value", declared_type: "TEXT" }],
      }],
    },
  };
}

export function demoTaxonomySqlSchema(alias: string): SqlSourceSchema {
  return {
    alias,
    objects: [
      {
        name: "taxa",
        object_type: "table",
        columns: [
          { name: "taxon_id", declared_type: "INTEGER" },
          { name: "parent_taxon_id", declared_type: "INTEGER" },
          { name: "rank", declared_type: "INTEGER" },
          { name: "geological_range", declared_type: "TEXT" },
        ],
      },
      {
        name: "taxon_names",
        object_type: "table",
        columns: [
          { name: "name_id", declared_type: "INTEGER" },
          { name: "taxon_id", declared_type: "INTEGER" },
          { name: "name_type", declared_type: "INTEGER" },
          { name: "name", declared_type: "TEXT" },
        ],
      },
    ],
  };
}

export const executeCustomSql = (
  sql: string,
  ownerId: string,
  maximumResultRows: number | null = 1000,
) =>
  call<OperationState>("execute_custom_taxonomy_sql", {
    request: { sql, maximum_result_rows: maximumResultRows },
    ownerId,
  }, () => demoCompletedOperation("taxonomy", "execute_custom_taxonomy_sql", {
    operation_id: null,
    changeset_size: 0,
    result_sets: [{
      statement_index: 1,
      columns: [{ name: "demo", declared_type: "TEXT" }],
      rows: [[{ type: "text", value: "Demo result" }]],
      truncated: false,
    }],
    messages: [{ statement_index: 1, affected_rows: null, message: "Query completed" }],
    script_saved: true,
    warnings: [],
  }));
export const exportCustomSqlQuery = (
  sql: string,
  statementIndex: number,
  destinationPath: string,
  ownerId: string,
) =>
  call<OperationState>("export_custom_taxonomy_query", {
    request: { sql, statement_index: statementIndex, destination_path: destinationPath },
    ownerId,
  }, () => demoCompletedOperation("taxonomy", "export_custom_taxonomy_query", {
    path: destinationPath,
    row_count: 1,
  }));
export const getCustomTaxonomySql = () =>
  call<string>("get_custom_taxonomy_sql", undefined, () => "SELECT * FROM taxa LIMIT 100;");
export const listCustomSqlInputs = () =>
  call<PersistentSqlInput[]>("list_custom_sql_inputs", undefined, () => []);
export const listCustomSqlDatabaseSchemas = () =>
  call<SqlSourceSchema[]>("list_custom_sql_database_schemas", undefined, () => [demoTaxonomySqlSchema("main")]);
export const addCustomSqlInput = (kind: "csv" | "sqlite", alias: string, path: string) =>
  call<AddSqlInputResult>("add_custom_sql_input", { request: { kind, alias, path } }, () => {
    const input = demoSqlInput(kind, alias, path);
    return { input, inputs: [input], warnings: [] };
  });
export const removeCustomSqlInput = (alias: string) =>
  call<RemoveSqlInputResult>("remove_custom_sql_input", { request: { alias } }, () => ({ inputs: [], warnings: [] }));
