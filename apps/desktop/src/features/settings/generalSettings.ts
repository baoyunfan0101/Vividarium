import type { GeneralSettings, ThemePreference } from "../../api/generalModel";

export function normalizeGeneralSettings(value: Partial<GeneralSettings>): GeneralSettings {
  const fallback: GeneralSettings = {
    theme: "dark",
    restore_tabs: true,
    recent_searches_limit: 10,
    csv_delimiter: ",",
    photos_taxon_name_parts: {
      sci_name: true,
      zh_name: true,
      en_name: true,
    },
    taxonomy_taxon_name_parts: {
      sci_name: true,
      zh_name: true,
      en_name: true,
    },
  };
  const theme = isTheme(value.theme) ? value.theme : fallback.theme;
  const recentSearchesLimit = Number.isInteger(value.recent_searches_limit)
    && value.recent_searches_limit! >= 1
    && value.recent_searches_limit! <= 50
    ? value.recent_searches_limit!
    : fallback.recent_searches_limit;
  return {
    theme,
    restore_tabs: typeof value.restore_tabs === "boolean" ? value.restore_tabs : fallback.restore_tabs,
    recent_searches_limit: recentSearchesLimit,
    csv_delimiter: isCsvDelimiter(value.csv_delimiter) ? value.csv_delimiter : fallback.csv_delimiter,
    photos_taxon_name_parts: normalizeTaxonNameParts(
      value.photos_taxon_name_parts,
      fallback.photos_taxon_name_parts,
    ),
    taxonomy_taxon_name_parts: normalizeTaxonNameParts(
      value.taxonomy_taxon_name_parts,
      fallback.taxonomy_taxon_name_parts,
    ),
  };
}

export function applyTheme(
  theme: ThemePreference,
  root: Pick<HTMLElement, "dataset"> = document.documentElement,
): void {
  if (theme === "system") delete root.dataset.theme;
  else root.dataset.theme = theme;
}

function isTheme(value: unknown): value is ThemePreference {
  return value === "system" || value === "light" || value === "dark";
}

function isCsvDelimiter(value: unknown): value is GeneralSettings["csv_delimiter"] {
  return value === "," || value === ";" || value === "\t" || value === "|";
}

function normalizeTaxonNameParts(
  value: Partial<GeneralSettings["photos_taxon_name_parts"]> | undefined,
  fallback: GeneralSettings["photos_taxon_name_parts"],
): GeneralSettings["photos_taxon_name_parts"] {
  const next = {
    sci_name: typeof value?.sci_name === "boolean" ? value.sci_name : fallback.sci_name,
    zh_name: typeof value?.zh_name === "boolean" ? value.zh_name : fallback.zh_name,
    en_name: typeof value?.en_name === "boolean" ? value.en_name : fallback.en_name,
  };
  return next.sci_name || next.zh_name || next.en_name ? next : fallback;
}
