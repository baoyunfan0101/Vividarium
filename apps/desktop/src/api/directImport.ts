import { call } from "./client";
import type { SqlSourceSchema } from "./customSql";
import { demoCompletedOperation, demoOperation, type OperationState } from "./tasks";
import type { TaxonomyImportResult } from "./taxonomyImport";

export type DirectImportDatabase = {
  source_path: string;
  schema: SqlSourceSchema;
};

export const inspectDirectImportDatabase = (sourcePath: string, ownerId: string) =>
  call<OperationState>("inspect_direct_import_database", { sourcePath, ownerId }, () => demoCompletedOperation(
    "direct_import",
    "inspect_direct_import_database",
    {
      source_path: sourcePath,
      schema: {
        alias: "direct_import",
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
              { name: "authority_year", declared_type: "TEXT" },
              { name: "source", declared_type: "TEXT" },
            ],
          },
        ],
      },
    },
  ));

export const applyDirectImport = (sourcePath: string, ownerId: string) =>
  call<OperationState>("apply_direct_import", { sourcePath, ownerId }, () => {
    const operation = demoOperation("direct_import", "Direct Import applied");
    operation.operation = "apply_direct_import";
    operation.result = {
      metadata: {
        source_path: sourcePath,
        taxa_count: 125000,
        taxon_names_count: 185000,
        imported_at: new Date().toISOString(),
      },
      warnings: [],
    } satisfies TaxonomyImportResult;
    return operation;
  });
