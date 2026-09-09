import { formatBytes } from "../../api/common.ts";
import type { Photo } from "../../api/photos";

export function formatPhotoModifiedAt(modifiedAtNs: number): string {
  const date = new Date(modifiedAtNs / 1_000_000);
  return Number.isNaN(date.getTime()) ? String(modifiedAtNs) : date.toLocaleString();
}

export function formatPhotoSummary(photo: Pick<Photo, "file_size" | "modified_at_ns">): string {
  return `${formatBytes(photo.file_size)} \u00b7 ${formatPhotoModifiedAt(photo.modified_at_ns)}`;
}

export function photoPaneHeaderLines(photo: Pick<Photo, "filename" | "file_size" | "modified_at_ns">) {
  return {
    filename: photo.filename,
    summary: formatPhotoSummary(photo),
  };
}
