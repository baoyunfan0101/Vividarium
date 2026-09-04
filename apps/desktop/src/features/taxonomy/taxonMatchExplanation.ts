import type { TaxonomyNameType } from "../../api/taxonomy";

type TaxonMatchedName = {
  name_type: TaxonomyNameType;
  name: string;
};

type NonAcceptedNameType = "synonym" | "zh_alias" | "en_alias";

export type TaxonMatchExplanation = {
  nameType: NonAcceptedNameType;
  label: string;
  name: string;
};

const nonAcceptedNameLabels: Record<NonAcceptedNameType, string> = {
  synonym: "Matched synonym",
  zh_alias: "Matched Chinese alias",
  en_alias: "Matched English alias",
};

const nameTypeOrder: NonAcceptedNameType[] = ["synonym", "zh_alias", "en_alias"];

export function taxonMatchExplanations(matches: TaxonMatchedName[]): TaxonMatchExplanation[] {
  const seen = new Set<string>();
  return nameTypeOrder.flatMap((nameType) => matches.flatMap((match) => {
    if (match.name_type !== nameType) return [];
    const key = `${nameType}\u0000${match.name}`;
    if (seen.has(key)) return [];
    seen.add(key);
    return [{ nameType, label: nonAcceptedNameLabels[nameType], name: match.name }];
  }));
}
