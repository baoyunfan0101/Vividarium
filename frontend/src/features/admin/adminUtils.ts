import type { OperationState, Photo, PhotoRootMetadata } from "../../api";
import type { RootRow } from "./types";

export function uniqueRoots(roots: string[]): string[] {
  return Array.from(new Set(roots));
}

export function mergeRootSelection(metadata: PhotoRootMetadata[], previousRows: RootRow[]): RootRow[] {
  const selected = new Set(previousRows.filter((row) => row.selected).map((row) => row.root.trim()));
  return metadata.map((row) => ({
    ...row,
    selected: selected.has(row.root)
  }));
}

export function moveSelectedRows(rows: RootRow[], direction: -1 | 1): RootRow[] {
  const nextRows = [...rows];
  const indexes = direction === -1
    ? nextRows.map((row, index) => row.selected ? index : -1).filter((index) => index > 0)
    : nextRows.map((row, index) => row.selected ? index : -1).filter((index) => index >= 0 && index < nextRows.length - 1).reverse();

  for (const index of indexes) {
    const target = index + direction;
    if (nextRows[target].selected) {
      continue;
    }
    const current = nextRows[index];
    nextRows[index] = nextRows[target];
    nextRows[target] = current;
  }

  return nextRows.map((row, index) => ({ ...row, sort_order: index }));
}

export function formatOperationAlert(label: string, result: unknown): string {
  if (hasUnmappedPhotos(result)) {
    const photos = result.result.unmapped_photos
      .map((photo) => `${photo.photo_id}: ${photo.root}/${photo.relative_path}`)
      .join("\n");
    return `${label} completed with ${result.result.unmapped} unmapped photos:\n${photos}`;
  }

  const count = operationCount(result);
  if (count !== null) {
    return `${label} succeeded. Changed ${count} rows.`;
  }

  return `${label} succeeded.`;
}

export function isConfirmationResponse(value: unknown): boolean {
  return Boolean(
    value &&
      typeof value === "object" &&
      "needs_confirmation" in value &&
      (value as { needs_confirmation?: unknown }).needs_confirmation === true
  );
}

export function operationLabel(operation: OperationState | undefined, fallback: string): string {
  if (!operation?.running || !operation.operation) {
    return `${fallback} running`;
  }
  return `${fallback} ${operation.operation}`;
}

export function hasUnmappedPhotos(value: unknown): value is {
  result: { unmapped: number; unmapped_photos: Photo[] };
} {
  if (!value || typeof value !== "object" || !("result" in value)) {
    return false;
  }
  const result = (value as { result?: unknown }).result;
  return Boolean(
    result &&
      typeof result === "object" &&
      "unmapped" in result &&
      Number((result as { unmapped: unknown }).unmapped) > 0 &&
      Array.isArray((result as { unmapped_photos?: unknown }).unmapped_photos),
  );
}

export function operationCount(value: unknown): number | null {
  if (!value || typeof value !== "object" || !("result" in value)) {
    return null;
  }
  const result = (value as { result?: unknown }).result;
  if (!result || typeof result !== "object") {
    return null;
  }
  const record = result as Record<string, unknown>;

  if (typeof record.changed === "number") {
    return record.changed;
  }
  if (typeof record.taxa_changed === "number") {
    return record.taxa_changed;
  }
  if (typeof record.processed === "number") {
    return record.processed;
  }
  if (typeof record.inserted === "number") {
    return record.inserted;
  }
  if (record.results && typeof record.results === "object") {
    return Object.values(record.results as Record<string, Record<string, unknown>>)
      .reduce((total, item) => total + ["new", "updated", "deleted", "inserted"].reduce((sum, key) => (
        sum + (typeof item[key] === "number" ? item[key] : 0)
      ), 0), 0);
  }

  return ["new", "updated", "deleted", "inserted", "mapped"].reduce((total, key) => (
    total + (typeof record[key] === "number" ? record[key] : 0)
  ), 0);
}

export function formatBytes(value: number | null | undefined): string {
  if (value == null) {
    return "unknown";
  }
  if (value < 1024) {
    return `${value} B`;
  }
  if (value < 1024 * 1024) {
    return `${(value / 1024).toFixed(1)} KB`;
  }
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}
