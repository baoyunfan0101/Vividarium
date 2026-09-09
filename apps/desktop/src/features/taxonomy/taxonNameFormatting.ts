import type { TaxonNameParts } from "../../api/general";
import type { TaxonDisplayNames } from "../../api/taxonomy";
import type { TaxonDisplaySummary } from "../../api/taxonomy";

export function formatTaxonName(
  names: TaxonDisplayNames,
  visibleNameParts: TaxonNameParts,
  fallback = "",
): string {
  const selected = [
    visibleNameParts.sci_name ? names.sci_name : null,
    visibleNameParts.zh_name ? names.zh_name : null,
    visibleNameParts.en_name ? names.en_name : null,
  ].filter((name): name is string => Boolean(name));
  return selected.join(" \u00b7 ") || fallback;
}

export function formatTaxonDisplaySummary(
  summary: TaxonDisplaySummary,
  nameParts: TaxonNameParts,
): string {
  return summary.items
    .map((item) => formatTaxonName(item.names, nameParts, `Taxon ${item.taxon_id}`))
    .join(" > ");
}

export function taxonDisplayShrinkWeight(index: number, itemCount: number): number {
  const distanceFromCurrent = itemCount - index - 1;
  if (distanceFromCurrent <= 0) return 1;
  if (distanceFromCurrent === 1) return 100;
  return 1000;
}
