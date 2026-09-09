import type { PhotoTaxonStatus } from "../../api/mapping";
import type { TaxonDisplaySummary } from "../../api/taxonomy";

export type PhotoTaxonDisplayState = {
  summary: TaxonDisplaySummary | null;
  mappingStatus: PhotoTaxonStatus | null;
};

export function statusBarMappingStatus(
  state: PhotoTaxonDisplayState | null,
): PhotoTaxonStatus | null {
  return state?.summary ? null : state?.mappingStatus ?? null;
}
