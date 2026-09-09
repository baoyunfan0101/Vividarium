import type {
  SaveTaxonNameGroupInput,
  TaxonNameDetail,
  TaxonomyNameType,
} from "../../api/taxonomy";

export const taxonNameGroupKinds = [
  "sci_name",
  "synonym",
  "zh_name",
  "zh_alias",
  "en_name",
  "en_alias",
] as const satisfies readonly TaxonomyNameType[];

export type TaxonNameGroupKind = (typeof taxonNameGroupKinds)[number];

export const taxonNameGroupLabels: Record<TaxonNameGroupKind, string> = {
  sci_name: "Science name",
  synonym: "Synonyms",
  zh_name: "Chinese name",
  zh_alias: "Chinese aliases",
  en_name: "English name",
  en_alias: "English aliases",
};

export type TaxonNameDraftRow = {
  nameId: number | null;
  name: string;
  authorityYear: string;
  source: string;
};

export function isPrimaryTaxonNameGroup(kind: TaxonNameGroupKind): boolean {
  return kind === "sci_name" || kind === "zh_name" || kind === "en_name";
}

export function acceptedTaxonNameGroup(kind: TaxonNameGroupKind): TaxonNameGroupKind {
  if (kind === "synonym") return "sci_name";
  if (kind === "zh_alias") return "zh_name";
  if (kind === "en_alias") return "en_name";
  return kind;
}

export function canPromoteTaxonName(kind: TaxonNameGroupKind): boolean {
  return kind === "synonym" || kind === "zh_alias" || kind === "en_alias";
}

export function canDeleteTaxonName(
  kind: TaxonNameGroupKind,
  hasAliases: boolean,
): boolean {
  if (kind === "sci_name") return false;
  if (kind === "zh_name" || kind === "en_name") return !hasAliases;
  return true;
}

export function createTaxonNameDraftRows(records: TaxonNameDetail[]): TaxonNameDraftRow[] {
  return records.map((record) => ({
    nameId: record.name_id,
    name: record.name,
    authorityYear: record.authority_year ?? "",
    source: record.source ?? "",
  }));
}

export function createBlankTaxonNameDraftRow(): TaxonNameDraftRow {
  return { nameId: null, name: "", authorityYear: "", source: "" };
}

export function buildTaxonNameGroupSaveInput(
  taxonId: number,
  kind: TaxonNameGroupKind,
  rows: TaxonNameDraftRow[],
): SaveTaxonNameGroupInput {
  const additions = rows.filter((row) => row.nameId === null).map((row) => {
    const name = row.name.trim();
    if (!name) throw new Error("Name is required.");
    return {
      name,
      authority_year: optionalText(row.authorityYear),
      source: optionalText(row.source),
    };
  });
  return {
    taxon_id: taxonId,
    name_type: kind,
    updates: rows.flatMap((row) => row.nameId === null ? [] : [{
      name_id: row.nameId,
      authority_year: optionalText(row.authorityYear),
      source: optionalText(row.source),
    }]),
    additions,
  };
}

function optionalText(value: string): string | null {
  const trimmed = value.trim();
  return trimmed || null;
}
