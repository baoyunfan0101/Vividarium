import type { TaxonNameParts } from "../../api/general";
import type { TaxonDisplaySummary } from "../../api/taxonomy";
import {
  formatTaxonDisplaySummary,
  formatTaxonName,
  taxonDisplayShrinkWeight,
} from "./taxonNameFormatting";

export function TaxonDisplayPath({
  summary,
  nameParts,
  className = "",
}: {
  summary: TaxonDisplaySummary;
  nameParts: TaxonNameParts;
  className?: string;
}) {
  const title = formatTaxonDisplaySummary(summary, nameParts);
  return (
    <span className={`taxon-display-path ${className}`.trim()} title={title}>
      {summary.items.map((item, index) => (
        <span
          className="taxon-display-segment"
          key={item.taxon_id}
          style={{ flexShrink: taxonDisplayShrinkWeight(index, summary.items.length) }}
        >
          <span className="taxon-display-name">
            {formatTaxonName(item.names, nameParts, `Taxon ${item.taxon_id}`)}
          </span>
          {index < summary.items.length - 1 ? <span className="taxon-display-separator">&gt;</span> : null}
        </span>
      ))}
    </span>
  );
}
