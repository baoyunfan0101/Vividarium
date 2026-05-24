import type { Photo } from "../api";

export function photoDisplayPath(photo: Photo): string {
  const separator = photo.root.includes("\\") ? "\\" : "/";
  const root = photo.root.endsWith("/") || photo.root.endsWith("\\")
    ? photo.root.slice(0, -1)
    : photo.root;
  return `${root}${separator}${photo.relative_path.replace(/[\\/]/g, separator)}`;
}

export function formatGps(photo: Photo): string {
  if (photo.latitude == null || photo.longitude == null) {
    return "-";
  }
  return `${photo.latitude.toFixed(6)}, ${photo.longitude.toFixed(6)}`;
}
