import type { PhotoTaxonStatus } from "../../api/mapping";

export function mappingStatusLabel(status: PhotoTaxonStatus): string {
  return status[0].toUpperCase() + status.slice(1);
}

export function MappingBadge({ status }: { status: PhotoTaxonStatus }) {
  return <span className={`mapping-badge ${status}`}>{mappingStatusLabel(status)}</span>;
}
