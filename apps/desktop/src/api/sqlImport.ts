import { call } from "./client";
import { demoSqlInput, demoTaxonomySqlSchema, type AddSqlInputResult, type PersistentSqlInput, type RemoveSqlInputResult, type SqlSourceSchema, type SqlStatementMessage } from "./customSql";
import { demoOperation, type OperationState } from "./tasks";
import type { TaxonomyImportResult } from "./taxonomyImport";

export type SqlImportExecutionResult = {
  statements_executed: number;
  messages: SqlStatementMessage[];
  script_saved: boolean;
  warnings: string[];
};
export type SqlImportIssue = {
  code: string;
  message: string;
  taxon_id: number | null;
  related_taxon_id: number | null;
  table: string | null;
  row_identifier: string | null;
};
export type SqlImportValidationResult = {
  valid: boolean;
  can_apply: boolean;
  taxa_count: number;
  name_counts: Array<{ name_type: string; count: number }>;
  normalization_changes: number;
  total_warning_count: number;
  total_error_count: number;
  warnings: SqlImportIssue[];
  errors: SqlImportIssue[];
};
export type ValidateSqlImportResult = {
  execution: SqlImportExecutionResult;
  validation: SqlImportValidationResult;
  warnings: string[];
  can_apply: boolean;
};
export const getSqlImportSql = () => call<string>("get_sql_import_sql", undefined, () => [
  "ATTACH DATABASE 'vividarium_sql_import.db' AS sql_import;",
  "CREATE TABLE sql_import.taxa AS SELECT * FROM biolib.taxa;",
  "CREATE TABLE sql_import.taxon_names AS SELECT * FROM biolib.taxon_names;",
].join("\n"));
export const listSqlImportInputs = () =>
  call<PersistentSqlInput[]>("list_sql_import_inputs", undefined, () => []);
export const listSqlImportDatabaseSchemas = () =>
  call<SqlSourceSchema[]>("list_sql_import_database_schemas", undefined, () => [demoTaxonomySqlSchema("taxonomy")]);
export const listSqlImportStagingSchemas = () =>
  call<SqlSourceSchema[]>("list_sql_import_staging_schemas", undefined, () => []);
export const addSqlImportInput = (kind: "csv" | "sqlite", alias: string, path: string) =>
  call<AddSqlInputResult>("add_sql_import_input", { request: { kind, alias, path } }, () => {
    const input = demoSqlInput(kind, alias, path);
    return { input, inputs: [input], warnings: [] };
  });
export const removeSqlImportInput = (alias: string) =>
  call<RemoveSqlInputResult>("remove_sql_import_input", { request: { alias } }, () => ({ inputs: [], warnings: [] }));
export const startSqlImportValidation = (sql: string, ownerId: string) =>
  call<OperationState>("start_sql_import_validation", { request: { sql }, ownerId }, () => {
    const operation = demoOperation("sql_import", "ready_to_apply");
    operation.operation = "validate_sql_import";
    operation.result = {
      execution: {
        statements_executed: sql.split(";").filter(Boolean).length,
        messages: [{ statement_index: 1, affected_rows: null, message: "Script completed" }],
        script_saved: true,
        warnings: [],
      },
      validation: {
        valid: true,
        can_apply: true,
        taxa_count: 125000,
        name_counts: [{ name_type: "sci_name", count: 125000 }, { name_type: "synonym", count: 60000 }],
        normalization_changes: 0,
        total_warning_count: 0,
        total_error_count: 0,
        warnings: [],
        errors: [],
      },
      warnings: [],
      can_apply: true,
    } satisfies ValidateSqlImportResult;
    return operation;
  });
export const applySqlImport = (ownerId: string) => call<OperationState>("apply_sql_import", { ownerId }, () => {
  const operation = demoOperation("sql_import", "SQL Import applied");
  operation.result = {
    metadata: {
      source_path: "demo-sql-import.db",
      taxa_count: 125000,
      taxon_names_count: 185000,
      imported_at: new Date().toISOString(),
    },
    warnings: [],
  } satisfies TaxonomyImportResult;
  return operation;
});
