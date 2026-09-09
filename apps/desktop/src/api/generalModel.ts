export type ThemePreference = "system" | "light" | "dark";

export type TaxonNameParts = {
  sci_name: boolean;
  zh_name: boolean;
  en_name: boolean;
};

export type GeneralSettings = {
  theme: ThemePreference;
  restore_tabs: boolean;
  recent_searches_limit: number;
  csv_delimiter: "," | ";" | "\t" | "|";
  photos_taxon_name_parts: TaxonNameParts;
  taxonomy_taxon_name_parts: TaxonNameParts;
};

export type WorkspaceTabKind =
  | "folders"
  | "photo-taxonomy"
  | "map"
  | "photo-history"
  | "mapping"
  | "taxonomy-search"
  | "formatted-update"
  | "custom-sql"
  | "taxonomy-history"
  | "settings"
  | "search-photos"
  | "taxon-photos"
  | "photo-detail"
  | "mapping-editor"
  | "taxon-detail";

export type WorkspaceSettingsSection =
  | "General"
  | "Storage"
  | "Photo Libraries"
  | "Taxonomy Databases"
  | "SQL Import"
  | "Direct Import"
  | "Naming"
  | "Map"
  | "Filename Parser"
  | "Synonym Splitter"
  | "About";

export type WorkspaceTab = {
  id: string;
  kind: WorkspaceTabKind;
  title: string;
  query: string | null;
  taxon_id: number | null;
  photo_id: number | null;
  settings_section: WorkspaceSettingsSection | null;
};

export type WorkspaceState = {
  opened_tabs: WorkspaceTab[];
  active_tab: string | null;
};

export const defaultGeneralSettings = (): GeneralSettings => ({
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
});
