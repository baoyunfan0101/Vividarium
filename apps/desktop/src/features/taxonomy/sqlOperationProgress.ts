import type { OperationState } from "../../api/tasks";

const stageLabels: Record<string, string> = {
  preparing_sql_sources: "Preparing input sources",
  preparing_input_sources: "Preparing input sources",
  executing_custom_sql: "Executing SQL",
  executing_sql: "Executing SQL",
  generating_custom_sql_changeset: "Generating changeset",
  validating_custom_sql_changes: "Validating changed taxonomy",
  recording_custom_sql_operation: "Recording operation",
  committing_custom_sql: "Committing changes",
  finalizing_custom_sql: "Finalizing",
  finalizing_staging_database: "Finalizing staging database",
  fingerprinting_staging: "Fingerprinting staging database",
  checking_staging_database: "Checking staging database",
  inspecting_staging_schema: "Inspecting staging schema",
  normalizing_names: "Normalizing names",
  validating_staging_taxonomy: "Validating staging taxonomy",
  building_candidate_taxa: "Building candidate taxa",
  building_candidate_names: "Building candidate names",
  validating_candidate_database: "Validating candidate database",
  loading_taxonomy_structure: "Loading taxonomy structure",
  checking_parent_cycles: "Checking parent cycles",
  checking_parent_relationships: "Checking parent relationships",
  checking_scientific_names: "Checking scientific names",
  checking_localized_names: "Checking localized names",
  checking_duplicate_names: "Checking duplicate names",
  checking_orphan_names: "Checking orphan names",
  checking_normalized_names: "Checking normalized names",
  ready_to_apply: "Ready to apply",
  validation_failed: "Validation failed",
};

export function sqlOperationProgress(operation: OperationState | null): string {
  const progress = operation?.progress;
  if (!progress) return operation?.state === "queued" ? "Waiting to start" : "";
  const label = stageLabels[progress.stage] ?? progress.stage.replace(/_/g, " ");
  const count = progress.current !== null && progress.total !== null
    ? progress.unit === "statements"
      ? `Statement ${progress.current} of ${progress.total}`
      : `${progress.current.toLocaleString()} / ${progress.total.toLocaleString()}${progress.unit ? ` ${progress.unit}` : ""}`
    : progress.current !== null
      ? `${progress.current.toLocaleString()}${progress.unit ? ` ${progress.unit}` : ""}`
      : "";
  const elapsed = operation?.started_at
    ? `${Math.max(0, Math.floor((Date.now() - Date.parse(operation.started_at)) / 1000))}s elapsed`
    : "";
  return [label, count, elapsed].filter(Boolean).join(" - ");
}
