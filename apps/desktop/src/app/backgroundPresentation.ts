import type { OperationState, OperationsStatus } from "../api/tasks";

const taskTitles: Record<string, string> = {
  validate_sql_import: "SQL import validation",
  apply_sql_import: "SQL import",
  apply_direct_import: "Direct import",
  photo_scan: "Photo scan",
  metadata_index: "Photo metadata index",
  photo_mapping: "Photo mapping",
  rename_from_taxonomy: "Rename photos",
  rename_directory_from_taxonomy: "Rename photos recursively",
  execute_custom_taxonomy_sql: "Custom SQL",
  export_custom_taxonomy_query: "Custom SQL export",
  inspect_direct_import_database: "Direct import inspection",
  preview_taxonomy_rows: "Formatted Update preview",
  apply_taxonomy_rows: "Formatted Update",
};

const stageLabels: Record<string, string> = {
  preparing_input_sources: "Preparing input sources",
  executing_sql: "Executing SQL",
  finalizing_staging_database: "Finalizing staging database",
  fingerprinting_staging: "Fingerprinting staging database",
  checking_staging_database: "Checking staging database",
  inspecting_staging_schema: "Inspecting staging schema",
  normalizing_names: "Normalizing names",
  validating_staging_taxonomy: "Validating staging taxonomy",
  loading_taxonomy_structure: "Loading taxonomy structure",
  checking_parent_cycles: "Checking parent cycles",
  checking_parent_relationships: "Checking parent relationships",
  checking_scientific_names: "Checking scientific names",
  checking_localized_names: "Checking localized names",
  checking_duplicate_names: "Checking duplicate names",
  checking_orphan_names: "Checking orphan names",
  checking_normalized_names: "Checking normalized names",
  building_candidate_taxa: "Building candidate taxa",
  building_candidate_names: "Building candidate names",
  validating_candidate_database: "Validating candidate database",
  ready_to_apply: "Ready to apply",
  validation_failed: "Validation failed",
  validation_could_not_be_completed: "Validation could not be completed",
  scanning_files: "Scanning files",
  updating_photo_index: "Updating photo index",
  photo_index_complete: "Photo index complete",
  indexing_photo_metadata: "Reading photo metadata",
  mapping_photos: "Matching photo names",
  renaming_photos: "Renaming photos",
  synchronizing_taxonomy: "Synchronizing taxonomy changes",
  no_active_photo_library: "No active Photo Library",
  validating_sql_import_candidate: "Validating SQL import candidate",
  applying_sql_import: "Applying SQL import",
  validating_direct_import_database: "Validating direct import database",
  applying_direct_import: "Applying direct import",
  executing_custom_sql: "Executing Custom SQL",
  exporting_custom_sql_query: "Exporting Custom SQL query",
  preparing_formatted_update: "Preparing Formatted Update",
  applying_formatted_update: "Applying Formatted Update",
};

export type BackgroundProgressMode = "none" | "determinate" | "indeterminate";

export function backgroundTaskTitle(operation: OperationState): string {
  const identifier = operation.operation ?? operation.task_kind ?? operation.module;
  return taskTitles[identifier] ?? formatIdentifier(identifier);
}

export function backgroundStageLabel(operation: OperationState): string {
  if (operation.state === "queued") return "Queued";
  if (operation.state === "completed") return "Completed";
  if (operation.state === "failed") return "Failed";
  const stage = operation.progress?.stage;
  return stage ? stageLabels[stage] ?? formatIdentifier(stage) : "Starting";
}

export function backgroundProgressMode(operation: OperationState): BackgroundProgressMode {
  if (operation.state !== "running") return "none";
  return operation.progress?.current != null && operation.progress.total != null
    ? "determinate"
    : "indeterminate";
}

export function backgroundProgressText(operation: OperationState): string | null {
  if (backgroundProgressMode(operation) !== "determinate") return null;
  const progress = operation.progress!;
  const current = progress.current!;
  const total = progress.total!;
  if (progress.unit === "bytes") {
    return `${formatBackgroundBytes(current)} / ${formatBackgroundBytes(total)}`;
  }
  const unit = progress.unit ? ` ${progress.unit}` : "";
  return `${current.toLocaleString()} / ${total.toLocaleString()}${unit}`;
}

export function backgroundElapsed(operation: OperationState, now: number): string {
  const startedAt = parseTaskTime(operation.started_at);
  if (startedAt === null) return "";
  const finishedAt = operation.state === "running"
    ? now
    : parseTaskTime(operation.finished_at);
  if (finishedAt === null) return "";
  return formatElapsed(finishedAt - startedAt);
}

export function formatElapsed(milliseconds: number): string {
  const totalSeconds = Math.max(0, Math.floor(milliseconds / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
  }
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

export function formatBackgroundBytes(value: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let amount = Math.max(0, value);
  let unitIndex = 0;
  while (amount >= 1024 && unitIndex < units.length - 1) {
    amount /= 1024;
    unitIndex += 1;
  }
  const digits = unitIndex === 0 || amount >= 100 ? 0 : 1;
  return `${amount.toFixed(digits)} ${units[unitIndex]}`;
}

export function backgroundTaskSections(operations: OperationsStatus): {
  active: OperationState[];
  recent: OperationState[];
} {
  const values = Object.values(operations);
  const active = values
    .filter((operation) => operation.state === "queued" || operation.state === "running")
    .sort((left, right) => taskTime(right.started_at) - taskTime(left.started_at));
  const recent = values
    .filter((operation) => operation.state === "completed" || operation.state === "failed")
    .sort((left, right) => taskTime(right.finished_at) - taskTime(left.finished_at))
    .slice(0, 10);
  return { active, recent };
}

export function backgroundActiveCount(operations: OperationsStatus): number {
  return Object.values(operations).filter(
    (operation) => operation.state === "queued" || operation.state === "running",
  ).length;
}

function parseTaskTime(value: string | null): number | null {
  if (!value) return null;
  const parsed = Date.parse(value.replace(" ", "T"));
  return Number.isFinite(parsed) ? parsed : null;
}

function taskTime(value: string | null): number {
  return parseTaskTime(value) ?? 0;
}

function formatIdentifier(value: string): string {
  const words = value.split("_").filter(Boolean).join(" ");
  return words.length === 0 ? "Background task" : words[0].toUpperCase() + words.slice(1);
}
