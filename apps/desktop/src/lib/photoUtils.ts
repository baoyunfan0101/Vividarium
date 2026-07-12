import type { Photo } from "../bridge";
import { displayPath } from "./pathUtils";

export function photoDisplayPath(photo: Photo): string {
  const separator = photo.root.includes("\\") ? "\\" : "/";
  const displayRoot = displayPath(photo.root);
  const root = displayRoot.endsWith("/") || displayRoot.endsWith("\\")
    ? displayRoot.slice(0, -1)
    : displayRoot;
  return `${root}${separator}${photo.relative_path.replace(/[\\/]/g, separator)}`;
}

export function formatGps(photo: Photo): string {
  if (photo.latitude == null || photo.longitude == null) {
    return "-";
  }
  return `${photo.latitude.toFixed(6)}, ${photo.longitude.toFixed(6)}`;
}
