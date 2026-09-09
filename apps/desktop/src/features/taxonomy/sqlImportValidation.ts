import type { SqlImportIssue } from "../../api/sqlImport";

export const SQL_IMPORT_VALIDATION_ISSUE_ROW_HEIGHT = 44;

export function sqlImportValidationIssueRow(issue: SqlImportIssue) {
  return {
    message: issue.message,
    context: sqlImportIssueContext(issue),
  };
}

function sqlImportIssueContext(issue: SqlImportIssue): string {
  const context = [];
  if (issue.taxon_id !== null) context.push(`Taxon ${issue.taxon_id}`);
  if (issue.related_taxon_id !== null) context.push(`Related taxon ${issue.related_taxon_id}`);
  if (context.length === 0) context.push(...[issue.table, issue.row_identifier].filter(Boolean));
  return context.join(" / ");
}
