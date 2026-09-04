import type { KeyboardEvent, ReactNode } from "react";
import {
  displayTaxon,
  type TaxonDisplayNames,
  type TaxonRank,
} from "../../api/taxonomy";
import { taxonCommonNameLine } from "./taxonCardNames";
import { selectionIntersectsElement } from "../../shared/selectableSurface";
import type { TaxonMatchExplanation } from "./taxonMatchExplanation";

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
  matchExplanations = [],
  onClick,
}: {
  taxon: TaxonCardTaxon;
  compact?: boolean;
  active?: boolean;
  actions?: ReactNode;
  matchExplanations?: TaxonMatchExplanation[];
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
        <span className="taxon-card-common-names">{taxonCommonNameLine(taxon.names)}</span>
        {matchExplanations.length > 0 ? (
          <span className="taxon-card-match-explanations">
            {matchExplanations.map((explanation) => (
              <small key={`${explanation.nameType}:${explanation.name}`}>
                {explanation.label} · {explanation.name}
              </small>
            ))}
          </span>
        ) : null}
      </div>
      {actions && <div className="taxon-card-actions">{actions}</div>}
    </article>
  );
}
