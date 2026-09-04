import type { KeyboardEvent, ReactNode } from "react";
import {
  displayTaxon,
  type TaxonDisplayNames,
  type TaxonRank,
} from "../../api/taxonomy";
import { taxonCommonNameLine } from "./taxonCardNames";
import { selectionIntersectsElement } from "../../shared/selectableSurface";

type TaxonCardTaxon = {
  taxon_id: number;
  rank: TaxonRank;
  names: TaxonDisplayNames;
};

export function TaxonCard({
  taxon,
  compact = false,
  active = false,
  actions,
  description,
  onClick,
}: {
  taxon: TaxonCardTaxon;
  compact?: boolean;
  active?: boolean;
  actions?: ReactNode;
  description?: string | null;
  onClick?: () => void;
}) {
  return (
    <article
      className={`taxon-card${compact ? " compact" : ""}${active ? " active" : ""}${onClick ? " clickable" : ""}`}
    >
      <div
        className="taxon-card-main selectable-content"
        role={onClick ? "button" : undefined}
        tabIndex={onClick ? 0 : undefined}
        onClick={onClick ? (event) => {
          if (!selectionIntersectsElement(event.currentTarget)) onClick();
        } : undefined}
        onKeyDown={onClick ? (event: KeyboardEvent<HTMLDivElement>) => {
          if (event.key !== "Enter" && event.key !== " ") return;
          event.preventDefault();
          onClick();
        } : undefined}
      >
        <span className="taxon-rank">{taxon.rank}</span>
        <strong>{displayTaxon(taxon)}</strong>
        <span>{taxonCommonNameLine(taxon.names)}</span>
        {description ? <small className="taxon-card-description">{description}</small> : null}
      </div>
      {actions && <div className="taxon-card-actions">{actions}</div>}
    </article>
  );
}
