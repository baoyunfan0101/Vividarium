import { call } from "./client";
import type { Page } from "./common";
import { getGeneralSettings } from "./general";
import { demoPhotos, type Photo } from "./photos";
import { demoCompletedOperation, type OperationState } from "./tasks";

export type TaxonRank = "kingdom" | "order" | "family" | "genus" | "species";
export type TaxonomyNameType =
  | "sci_name"
  | "synonym"
  | "zh_name"
  | "zh_alias"
  | "en_name"
  | "en_alias";
export type TaxonDisplayNames = { sci_name: string | null; zh_name: string | null; en_name: string | null };
export type TaxonBreadcrumbItem = { taxon_id: number; rank: TaxonRank; names: TaxonDisplayNames };
export type TaxonSummary = {
  taxon_id: number;
  rank: TaxonRank;
  breadcrumb: TaxonBreadcrumbItem[];
  names: TaxonDisplayNames;
};
export type TaxonDisplayItem = { taxon_id: number; rank: TaxonRank; names: TaxonDisplayNames };
export type TaxonDisplaySummary = { current_rank: TaxonRank; items: TaxonDisplayItem[] };
export type TaxonNameDetail = {
  name_id: number;
  name: string;
  authority_year: string | null;
  source: string | null;
};
export type TaxonDetail = {
  taxon_id: number;
  rank: TaxonRank;
  parent_taxon_id: number | null;
  breadcrumb: TaxonBreadcrumbItem[];
  geological_range: string | null;
  names: {
    sci_name: TaxonNameDetail | null;
    synonyms: TaxonNameDetail[];
    zh_name: TaxonNameDetail | null;
    zh_aliases: TaxonNameDetail[];
    en_name: TaxonNameDetail | null;
    en_aliases: TaxonNameDetail[];
  };
};
export type TaxonNameMatch = { name_id: number; name_type: TaxonomyNameType; name: string };
export type TaxonSearchResult = {
  taxon_id: number;
  rank: TaxonRank;
  names: TaxonDisplayNames;
  matches: TaxonNameMatch[];
};
export type TaxonSuggestion = {
  taxon_id: number;
  rank: TaxonRank;
  names: TaxonDisplayNames;
  matches: TaxonNameMatch[];
};
export type TaxonChild = {
  taxon_id: number;
  rank: TaxonRank;
  names: TaxonDisplayNames;
};

export type TaxonInputRow = {
  kingdom?: string | null;
  order?: string | null;
  family?: string | null;
  genus?: string | null;
  species?: string | null;
  authority_year?: string | null;
  synonyms?: string[];
  zh_name?: string | null;
  zh_alias?: string[];
  en_name?: string | null;
  en_alias?: string[];
  geological_range?: string | null;
  source?: string | null;
};
export type TaxonNameActionInput = {
  taxon_id: number;
  name_id: number;
};
export type TaxonNameMetadataInput = {
  name_id: number;
  authority_year: string | null;
  source: string | null;
};
export type NewTaxonNameInput = {
  name: string;
  authority_year: string | null;
  source: string | null;
};
export type SaveTaxonNameGroupInput = {
  taxon_id: number;
  name_type: TaxonomyNameType;
  updates: TaxonNameMetadataInput[];
  additions: NewTaxonNameInput[];
};
export type TaxonRowOutcome = {
  row_number: number;
  operation_types: string[];
  message: string;
  target: TaxonSummary | null;
  parent: TaxonSummary | null;
  candidates: TaxonSummary[];
  changes: Array<{
    kind: string;
    field: string;
    old_value: string | null;
    new_value: string | null;
  }>;
};
export type TaxonomyPreviewResult = { delimiter: string; encoding: string; rows: TaxonRowOutcome[] };
export type FormattedUpdatePreviewResult = TaxonomyPreviewResult & { preview_id: string };
export type TaxonomyOperationResult = TaxonomyPreviewResult & {
  operation_id: number;
  total_rows: number;
  succeeded_rows: number;
  failed_rows: number;
};

type DemoTaxonDefinition = {
  taxon_id: number;
  parent_taxon_id: number | null;
  rank: TaxonRank;
  scientific: string;
  english: string;
};

const demoTaxonomy: DemoTaxonDefinition[] = [
  { taxon_id: 1, parent_taxon_id: null, rank: "kingdom", scientific: "Animalia", english: "Animals" },
  { taxon_id: 2, parent_taxon_id: 1, rank: "order", scientific: "Carnivora", english: "Carnivorans" },
  { taxon_id: 3, parent_taxon_id: 2, rank: "family", scientific: "Canidae", english: "Canids" },
  { taxon_id: 4, parent_taxon_id: 3, rank: "genus", scientific: "Canis", english: "Canis" },
  { taxon_id: 5, parent_taxon_id: 2, rank: "family", scientific: "Felidae", english: "Felids" },
  { taxon_id: 6, parent_taxon_id: 5, rank: "genus", scientific: "Panthera", english: "Panthera" },
  { taxon_id: 7, parent_taxon_id: 3, rank: "genus", scientific: "Vulpes", english: "Vulpes" },
  { taxon_id: 8, parent_taxon_id: 2, rank: "family", scientific: "Ursidae", english: "Bears" },
  { taxon_id: 9, parent_taxon_id: 8, rank: "genus", scientific: "Ursus", english: "Ursus" },
  { taxon_id: 1001, parent_taxon_id: 4, rank: "species", scientific: "Canis lupus", english: "Wolf" },
  { taxon_id: 1002, parent_taxon_id: 6, rank: "species", scientific: "Panthera leo", english: "Lion" },
  { taxon_id: 1003, parent_taxon_id: 7, rank: "species", scientific: "Vulpes vulpes", english: "Red fox" },
  { taxon_id: 1004, parent_taxon_id: 9, rank: "species", scientific: "Ursus arctos", english: "Brown bear" },
];

const demoTaxonomyById = new Map(demoTaxonomy.map((taxon) => [taxon.taxon_id, taxon]));

export const demoTaxa: TaxonSearchResult[] = demoTaxonomy
  .filter((taxon) => taxon.rank === "species")
  .map(demoTaxon);

function demoTaxon(taxon: DemoTaxonDefinition): TaxonSearchResult {
  return {
    taxon_id: taxon.taxon_id,
    rank: taxon.rank,
    names: { sci_name: taxon.scientific, zh_name: null, en_name: taxon.english },
    matches: [{ name_id: taxon.taxon_id * 10, name_type: "sci_name", name: taxon.scientific }],
  };
}

function demoBreadcrumb(taxon: DemoTaxonDefinition): TaxonBreadcrumbItem[] {
  const breadcrumb: TaxonBreadcrumbItem[] = [];
  let parentTaxonId = taxon.parent_taxon_id;
  while (parentTaxonId !== null) {
    const parent = demoTaxonomyById.get(parentTaxonId);
    if (!parent) throw new Error(`taxon ${taxon.taxon_id} references missing parent ${parentTaxonId}`);
    breadcrumb.push({
      taxon_id: parent.taxon_id,
      rank: parent.rank,
      names: { sci_name: parent.scientific, zh_name: null, en_name: parent.english },
    });
    parentTaxonId = parent.parent_taxon_id;
  }
  return breadcrumb.reverse();
}

const demoTaxonDetails = new Map<number, TaxonDetail>(demoTaxonomy.map((taxon) => [
  taxon.taxon_id,
  {
    taxon_id: taxon.taxon_id,
    rank: taxon.rank,
    parent_taxon_id: taxon.parent_taxon_id,
    breadcrumb: demoBreadcrumb(taxon),
    geological_range: "Recent",
    names: {
      sci_name: {
        name_id: taxon.taxon_id * 10,
        name: taxon.scientific,
        authority_year: null,
        source: "Demo",
      },
      synonyms: taxon.taxon_id === 1001 ? [{
        name_id: taxon.taxon_id * 10 + 2,
        name: "Canis lycaon",
        authority_year: "Schreber, 1775",
        source: "Demo synonym index",
      }] : [],
      zh_name: null,
      zh_aliases: [],
      en_name: {
        name_id: taxon.taxon_id * 10 + 1,
        name: taxon.english,
        authority_year: null,
        source: "Demo",
      },
      en_aliases: taxon.taxon_id === 1003 ? [{
        name_id: taxon.taxon_id * 10 + 2,
        name: "Common fox",
        authority_year: null,
        source: "Demo vernacular index",
      }] : [],
    },
  },
]));
let nextDemoTaxonNameId = demoTaxonomy.reduce((maximum, taxon) => Math.max(maximum, taxon.taxon_id * 10 + 2), 0) + 1;
let nextDemoTaxonomyOperationId = 1000;

const demoChildrenByParent = new Map<number, TaxonChild[]>();
for (const taxon of demoTaxonomy) {
  if (taxon.parent_taxon_id === null) continue;
  const children = demoChildrenByParent.get(taxon.parent_taxon_id) ?? [];
  children.push({
    taxon_id: taxon.taxon_id,
    rank: taxon.rank,
    names: { sci_name: taxon.scientific, zh_name: null, en_name: taxon.english },
  });
  demoChildrenByParent.set(taxon.parent_taxon_id, children);
}
const demoRankOrder: Record<TaxonRank, number> = { kingdom: 1, order: 2, family: 3, genus: 4, species: 5 };
for (const children of demoChildrenByParent.values()) {
  children.sort((left, right) => demoRankOrder[left.rank] - demoRankOrder[right.rank] || left.taxon_id - right.taxon_id);
}

export function displayTaxon(summary: { taxon_id: number; names: TaxonDisplayNames }): string {
  return summary.names.sci_name ?? summary.names.zh_name ?? summary.names.en_name ?? `Taxon ${summary.taxon_id}`;
}

export function displayTaxonDetail(detail: Pick<TaxonDetail, "taxon_id" | "names">): string {
  return detail.names.sci_name?.name
    ?? detail.names.zh_name?.name
    ?? detail.names.en_name?.name
    ?? `Taxon ${detail.taxon_id}`;
}

export function demoTaxonSummary(taxon: TaxonSearchResult): TaxonSummary {
  return {
    taxon_id: taxon.taxon_id,
    rank: taxon.rank,
    breadcrumb: demoTaxonDetails.get(taxon.taxon_id)?.breadcrumb ?? [],
    names: taxon.names,
  };
}

export function demoTaxonDisplaySummary(taxonId: number): TaxonDisplaySummary | null {
  const current = demoTaxonomy.find((taxon) => taxon.taxon_id === taxonId);
  if (!current) return null;
  const lineage: DemoTaxonDefinition[] = [];
  let item: DemoTaxonDefinition | undefined = current;
  while (item) {
    lineage.push(item);
    item = item.parent_taxon_id === null
      ? undefined
      : demoTaxonomy.find((taxon) => taxon.taxon_id === item!.parent_taxon_id);
  }
  const items = lineage
    .reverse()
    .filter((taxon) => current.rank === "kingdom" || current.rank === "order"
      ? taxon.taxon_id === current.taxon_id
      : demoRankOrder[taxon.rank] >= demoRankOrder.family)
    .map((taxon) => ({
      taxon_id: taxon.taxon_id,
      rank: taxon.rank,
      names: { sci_name: taxon.scientific, zh_name: null, en_name: taxon.english },
    }));
  return { current_rank: current.rank, items };
}

export const searchTaxa = (query: string, limit = 80) =>
  call<TaxonSearchResult[]>("search_taxa", { query, limit }, () =>
    demoTaxa.flatMap((taxon) => {
      const result = demoSearchResult(taxon, query);
      return result ? [result] : [];
    }).slice(0, limit));
export const suggestTaxa = (query: string, limit = 10) =>
  call<TaxonSuggestion[]>("suggest_taxa", { query, limit }, () =>
    demoTaxa
      .flatMap((taxon) => {
        const result = demoSearchResult(taxon, query);
        return result ? [result] : [];
      })
      .slice(0, limit)
      .map((taxon) => ({ ...taxon })));
export const suggestPhotoTaxa = (query: string, limit = 10) =>
  call<TaxonSuggestion[]>("suggest_photo_taxa", { query, limit }, () => suggestTaxa(query, limit));
export const getTaxonDetail = (taxonId: number) =>
  call<TaxonDetail>("get_taxon_detail", { taxonId }, () => {
    const detail = demoTaxonDetails.get(taxonId);
    if (!detail) throw new Error(`taxon ${taxonId} not found`);
    return detail;
  });
export const getTaxonDisplaySummary = (taxonId: number) =>
  call<TaxonDisplaySummary>("get_taxon_display_summary", { taxonId }, () => {
    const summary = demoTaxonDisplaySummary(taxonId);
    if (!summary) throw new Error(`taxon ${taxonId} not found`);
    return summary;
  });
export const listTaxonChildren = (taxonId: number, cursor: string | null = null, limit = 80) =>
  call<Page<TaxonChild>>("list_taxon_children", { taxonId, cursor, limit }, () => {
    const cursorPrefix = `demo-taxonomy-children:${taxonId}:`;
    const cursorOffset = cursor && cursor.length > 0 && cursor.startsWith(cursorPrefix)
      ? cursor.slice(cursorPrefix.length)
      : null;
    const offset = cursorOffset !== null && /^\d+$/.test(cursorOffset)
      ? Number(cursorOffset)
      : cursor ? Number.NaN : 0;
    if (!Number.isSafeInteger(offset) || offset < 0) throw new Error("invalid taxonomy cursor");
    const pageLimit = Math.min(Math.max(Math.trunc(limit), 1), 500);
    const children = demoChildrenByParent.get(taxonId) ?? [];
    const items = children.slice(offset, offset + pageLimit);
    const nextOffset = offset + items.length;
    return {
      items,
      next_cursor: nextOffset < children.length ? `${cursorPrefix}${nextOffset}` : null,
    };
  });
export const listTaxonPhotos = (taxonId: number, cursor: string | null = null, limit = 80) =>
  call<Page<Photo>>("list_taxon_photos", { taxonId, cursor, limit }, () => ({
    items: demoPhotos.slice((taxonId % 4) * 8, (taxonId % 4) * 8 + limit),
    next_cursor: null,
  }));
export const promoteTaxonName = (input: TaxonNameActionInput) =>
  call<void>("promote_taxon_name", { input }, () => demoPromoteTaxonName(input));
export const saveTaxonNameGroup = (input: SaveTaxonNameGroupInput) =>
  call<void>("save_taxon_name_group", { input }, () => demoSaveTaxonNameGroup(input));
export const deleteTaxonName = (input: TaxonNameActionInput) =>
  call<void>("delete_taxon_name", { input }, () => demoDeleteTaxonName(input));
export const deleteTaxon = (taxonId: number) =>
  call<void>("delete_taxon", { taxonId }, () => demoDeleteTaxon(taxonId));

function demoSearchResult(taxon: TaxonSearchResult, query: string): TaxonSearchResult | null {
  const normalized = query.trim().toLowerCase();
  if (!normalized) return null;
  const detail = demoTaxonDetails.get(taxon.taxon_id);
  if (!detail) return null;
  const candidates: Array<[TaxonomyNameType, TaxonNameDetail]> = [];
  if (detail.names.sci_name) candidates.push(["sci_name", detail.names.sci_name]);
  detail.names.synonyms.forEach((name) => candidates.push(["synonym", name]));
  if (detail.names.zh_name) candidates.push(["zh_name", detail.names.zh_name]);
  detail.names.zh_aliases.forEach((name) => candidates.push(["zh_alias", name]));
  if (detail.names.en_name) candidates.push(["en_name", detail.names.en_name]);
  detail.names.en_aliases.forEach((name) => candidates.push(["en_alias", name]));
  const matches = candidates
    .filter(([, name]) => name.name.toLowerCase().includes(normalized))
    .map(([nameType, name]) => ({
      name_id: name.name_id,
      name_type: nameType,
      name: name.name,
    }));
  return matches.length > 0 ? { ...taxon, matches } : null;
}

type DemoNameGroup = {
  acceptedType: "sci_name" | "zh_name" | "en_name";
  aliasType: "synonym" | "zh_alias" | "en_alias";
  accepted: () => TaxonNameDetail | null;
  setAccepted: (name: TaxonNameDetail | null) => void;
  aliases: TaxonNameDetail[];
};
type DemoNameLocation = {
  group: DemoNameGroup;
  name: TaxonNameDetail;
  nameType: TaxonomyNameType;
  aliasIndex: number | null;
};

function demoPromoteTaxonName(input: TaxonNameActionInput): void {
  const detail = requireDemoTaxonDetail(input.taxon_id);
  const location = findDemoName(detail, input.name_id);
  if (!location) throw new Error(`name ${input.name_id} for taxon ${input.taxon_id} not found`);
  if (location.aliasIndex === null) throw new Error("the selected name is already accepted");
  const accepted = location.group.accepted();
  if (!accepted) throw new Error(`taxon ${input.taxon_id} has no ${location.group.acceptedType} to exchange`);

  location.group.aliases[location.aliasIndex] = accepted;
  location.group.setAccepted(location.name);
  refreshDemoTaxonViews(input.taxon_id);
}

function demoSaveTaxonNameGroup(input: SaveTaxonNameGroupInput): void {
  const detail = requireDemoTaxonDetail(input.taxon_id);
  const group = demoNameGroupByType(detail, input.name_type);
  const seenIds = new Set<number>();
  for (const update of input.updates) {
    if (seenIds.has(update.name_id)) throw new Error(`name ${update.name_id} is included more than once`);
    seenIds.add(update.name_id);
    const location = findDemoName(detail, update.name_id);
    if (!location) throw new Error(`name ${update.name_id} for taxon ${input.taxon_id} not found`);
    if (location.nameType !== input.name_type) {
      throw new Error(`name ${update.name_id} is ${location.nameType}, not ${input.name_type}`);
    }
  }

  if (input.additions.length > 0) {
    const accepted = group.accepted();
    if (input.name_type === group.acceptedType && accepted) {
      throw new Error(`taxon ${input.taxon_id} already has ${input.name_type}`);
    }
    if (input.name_type === group.acceptedType && input.additions.length > 1) {
      throw new Error(`${input.name_type} accepts only one name`);
    }
    if (input.name_type === group.aliasType && !accepted) {
      throw new Error(`add ${group.acceptedType} before adding ${group.aliasType} records`);
    }
  }

  const normalizedAdditions = input.additions.map((addition) => ({
    name: normalizeDemoName(addition.name),
    authority_year: demoText(addition.authority_year),
    source: demoText(addition.source),
  }));
  const seenNames = new Set<string>();
  for (const addition of normalizedAdditions) {
    if (!addition.name) throw new Error("taxonomy name must not be blank");
    const normalized = addition.name.toLowerCase();
    if (seenNames.has(normalized)) throw new Error(`taxonomy name '${addition.name}' is included more than once`);
    seenNames.add(normalized);
    const allGroupNames = [group.accepted(), ...group.aliases].filter(Boolean) as TaxonNameDetail[];
    if (allGroupNames.some((name) => name.name.toLowerCase() === normalized)) {
      throw new Error(`taxonomy name '${addition.name}' already exists in this name group`);
    }
    if (detail.rank === "species" && (input.name_type === "sci_name" || input.name_type === "synonym")) {
      const parentName = detail.parent_taxon_id === null
        ? null
        : demoTaxonDetails.get(detail.parent_taxon_id)?.names.sci_name?.name ?? null;
      if (addition.name.split(/\s+/)[0] !== parentName) {
        throw new Error(`species scientific name '${addition.name}' does not start with parent genus '${parentName ?? ""}'`);
      }
    }
  }

  for (const update of input.updates) {
    const location = findDemoName(detail, update.name_id);
    if (!location) continue;
    location.name.authority_year = demoText(update.authority_year);
    location.name.source = demoText(update.source);
  }
  for (const addition of normalizedAdditions) {
    const record: TaxonNameDetail = {
      name_id: nextDemoTaxonNameId++,
      name: addition.name,
      authority_year: addition.authority_year,
      source: addition.source,
    };
    if (input.name_type === group.acceptedType) group.setAccepted(record);
    else group.aliases.push(record);
  }
  refreshDemoTaxonViews(input.taxon_id);
}

function demoDeleteTaxonName(input: TaxonNameActionInput): void {
  const detail = requireDemoTaxonDetail(input.taxon_id);
  const location = findDemoName(detail, input.name_id);
  if (!location) throw new Error(`name ${input.name_id} for taxon ${input.taxon_id} not found`);
  if (location.nameType === "sci_name") throw new Error("the unique sci_name cannot be deleted");
  if (location.aliasIndex === null) {
    location.group.setAccepted(null);
  } else {
    location.group.aliases.splice(location.aliasIndex, 1);
  }
  refreshDemoTaxonViews(input.taxon_id);
}

function demoDeleteTaxon(taxonId: number): void {
  const detail = requireDemoTaxonDetail(taxonId);
  if ((demoChildrenByParent.get(taxonId)?.length ?? 0) > 0) {
    throw new Error(`taxon ${taxonId} cannot be deleted because it has child taxa`);
  }

  if (detail.parent_taxon_id !== null) {
    const siblings = demoChildrenByParent.get(detail.parent_taxon_id);
    const childIndex = siblings?.findIndex((child) => child.taxon_id === taxonId) ?? -1;
    if (siblings && childIndex >= 0) siblings.splice(childIndex, 1);
    if (siblings?.length === 0) demoChildrenByParent.delete(detail.parent_taxon_id);
  }
  demoChildrenByParent.delete(taxonId);
  demoTaxonDetails.delete(taxonId);
  demoTaxonomyById.delete(taxonId);
  const definitionIndex = demoTaxonomy.findIndex((taxon) => taxon.taxon_id === taxonId);
  if (definitionIndex >= 0) demoTaxonomy.splice(definitionIndex, 1);
  const searchIndex = demoTaxa.findIndex((taxon) => taxon.taxon_id === taxonId);
  if (searchIndex >= 0) demoTaxa.splice(searchIndex, 1);
}

function requireDemoTaxonDetail(taxonId: number): TaxonDetail {
  const detail = demoTaxonDetails.get(taxonId);
  if (!detail) throw new Error(`taxon ${taxonId} not found`);
  return detail;
}

function demoNameGroups(detail: TaxonDetail): DemoNameGroup[] {
  return [
    {
      acceptedType: "sci_name",
      aliasType: "synonym",
      accepted: () => detail.names.sci_name,
      setAccepted: (name) => { detail.names.sci_name = name; },
      aliases: detail.names.synonyms,
    },
    {
      acceptedType: "zh_name",
      aliasType: "zh_alias",
      accepted: () => detail.names.zh_name,
      setAccepted: (name) => { detail.names.zh_name = name; },
      aliases: detail.names.zh_aliases,
    },
    {
      acceptedType: "en_name",
      aliasType: "en_alias",
      accepted: () => detail.names.en_name,
      setAccepted: (name) => { detail.names.en_name = name; },
      aliases: detail.names.en_aliases,
    },
  ];
}

function demoNameGroupByType(detail: TaxonDetail, nameType: TaxonomyNameType): DemoNameGroup {
  const group = demoNameGroups(detail).find((candidate) => (
    candidate.acceptedType === nameType || candidate.aliasType === nameType
  ));
  if (!group) throw new Error(`invalid taxonomy name type: ${nameType}`);
  return group;
}

function findDemoName(detail: TaxonDetail, nameId: number): DemoNameLocation | null {
  for (const group of demoNameGroups(detail)) {
    const accepted = group.accepted();
    if (accepted?.name_id === nameId) {
      return { group, name: accepted, nameType: group.acceptedType, aliasIndex: null };
    }
    const aliasIndex = group.aliases.findIndex((name) => name.name_id === nameId);
    if (aliasIndex >= 0) {
      return { group, name: group.aliases[aliasIndex], nameType: group.aliasType, aliasIndex };
    }
  }
  return null;
}

function refreshDemoTaxonViews(taxonId: number): void {
  const detail = requireDemoTaxonDetail(taxonId);
  const names: TaxonDisplayNames = {
    sci_name: detail.names.sci_name?.name ?? null,
    zh_name: detail.names.zh_name?.name ?? null,
    en_name: detail.names.en_name?.name ?? null,
  };
  const result = demoTaxa.find((taxon) => taxon.taxon_id === taxonId);
  if (result) {
    result.names = { ...names };
    result.matches = detail.names.sci_name
      ? [{ name_id: detail.names.sci_name.name_id, name_type: "sci_name", name: detail.names.sci_name.name }]
      : [];
  }
  for (const children of demoChildrenByParent.values()) {
    const child = children.find((candidate) => candidate.taxon_id === taxonId);
    if (child) child.names = { ...names };
  }
  for (const candidate of demoTaxonDetails.values()) {
    const item = candidate.breadcrumb.find((ancestor) => ancestor.taxon_id === taxonId);
    if (item) item.names = { ...names };
  }
}

function normalizeDemoName(value: string): string {
  return value.trim().replace(/\s+/g, " ");
}

function demoText(value: string | null): string | null {
  const normalized = value?.trim() ?? "";
  return normalized.length > 0 ? normalized : null;
}

export const getTaxonomyTemplate = () =>
  call<string>("get_taxonomy_formatted_update_template", undefined, async () => {
    const { csv_delimiter: delimiter } = await getGeneralSettings();
    return ["kingdom", "order", "family", "genus", "species", "authority_year", "synonyms", "zh_name", "zh_alias", "en_name", "en_alias", "geological_range", "source"].join(delimiter) + "\n";
  });
export const parseTaxonomyCsv = (input: string) =>
  call<TaxonInputRow[]>("parse_taxonomy_input_csv", { input }, async () => {
    const { csv_delimiter: delimiter } = await getGeneralSettings();
    return parseDemoTaxonomyCsv(input, delimiter);
  });

let demoFormattedUpdatePreview: {
  previewId: string;
  result: TaxonomyPreviewResult;
} | null = null;

export const previewTaxonomyRows = (rows: TaxonInputRow[], ownerId: string) =>
  call<OperationState>("preview_taxonomy_rows", { rows, ownerId }, async () => {
    const { csv_delimiter: delimiter } = await getGeneralSettings();
    const previewId = `demo-preview-${Date.now()}`;
    const result: TaxonomyPreviewResult = {
      delimiter,
      encoding: "UTF-8",
      rows: rows.map((row, index) => ({
        row_number: index + 1,
        operation_types: ["new_name"],
        message: "Ready to apply",
        target: null,
        parent: null,
        candidates: [],
        changes: [{ kind: "append_name", field: "species", old_value: null, new_value: row.species ?? null }],
      })),
    };
    demoFormattedUpdatePreview = { previewId, result };
    return demoCompletedOperation("taxonomy", "preview_taxonomy_rows", {
      ...result,
      preview_id: previewId,
    });
  });
export const applyTaxonomyRows = (previewId: string, ownerId: string) =>
  call<OperationState>("apply_taxonomy_rows", { previewId, ownerId }, () => {
    if (demoFormattedUpdatePreview?.previewId !== previewId) {
      throw new Error("Formatted update preview is no longer current; preview again");
    }
    const result = demoFormattedUpdatePreview.result;
    demoFormattedUpdatePreview = null;
    return demoCompletedOperation("taxonomy", "apply_taxonomy_rows", {
      ...result,
      operation_id: 100,
      total_rows: result.rows.length,
      succeeded_rows: result.rows.length,
      failed_rows: 0,
    });
  });

function parseDemoTaxonomyCsv(input: string, delimiter: string): TaxonInputRow[] {
  const records = parseDemoCsvRecords(input, delimiter);
  if (records.length < 2) return [];
  const headers = records[0];
  return records.slice(1).filter((values) => values.some(Boolean)).map((values) => {
    const row: Record<string, unknown> = {};
    headers.forEach((header, index) => {
      const value = values[index] ?? "";
      row[header] = ["synonyms", "zh_alias", "en_alias"].includes(header)
        ? value.split(";").filter(Boolean)
        : value || null;
    });
    return row as TaxonInputRow;
  });
}

function parseDemoCsvRecords(input: string, delimiter: string): string[][] {
  const records: string[][] = [];
  let record: string[] = [];
  let field = "";
  let quoted = false;
  for (let index = 0; index < input.length; index += 1) {
    const character = input[index];
    if (quoted) {
      if (character === '"' && input[index + 1] === '"') {
        field += '"';
        index += 1;
      } else if (character === '"') quoted = false;
      else field += character;
    } else if (character === '"' && field.length === 0) quoted = true;
    else if (character === delimiter) {
      record.push(field);
      field = "";
    } else if (character === "\n") {
      record.push(field);
      records.push(record);
      record = [];
      field = "";
    } else if (character !== "\r") field += character;
  }
  if (field.length > 0 || record.length > 0) {
    record.push(field);
    records.push(record);
  }
  return records;
}
