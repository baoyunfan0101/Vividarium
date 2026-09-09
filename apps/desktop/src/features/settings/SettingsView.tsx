import {
  ArrowDown,
  ArrowUp,
  BugPlay,
  CaseSensitive,
  ChevronDown,
  ChevronRight,
  Database,
  FolderCog,
  FolderInput,
  FolderOpen,
  Info,
  Library,
  Map,
  Plug,
  Save,
  SlidersHorizontal,
  Trash2,
  type LucideIcon,
} from "lucide-react";
import { Fragment, useEffect, useRef, useState, type ReactNode } from "react";
import {
  updateGeneralSettings,
  type GeneralSettings as GeneralSettingsValue,
  type WorkspaceSettingsSection,
} from "../../api/general";
import { errorMessage } from "../../api/common";
import {
  getDatabaseLocations,
  openPathInFileManager,
  relocateTaxonomyDatabase,
  setDefaultPhotoLibraryDatabaseDirectory,
  setDefaultTaxonomyDatabaseDirectory,
  type DatabaseLocations,
} from "../../api/storage";
import { getMapSettings, setMapSettings, type MapSettings } from "../../api/map";
import type { OperationState } from "../../api/tasks";
import { getTaxonomyImportMetadata, type TaxonomyImportMetadata } from "../../api/taxonomyImport";
import {
  getNamingHookSettings,
  getNamingHookTemplates,
  getNamingHookTestCases,
  getPhotoFilenameFormatSettings,
  getPhotoNameMatchSettings,
  getTaxonomyNameSeparator,
  runNamingHookTests,
  saveNamingHook,
  setPhotoFilenameFormatSettings,
  setPhotoNameMatchSettings,
  setTaxonomyNameSeparator,
  type NamingHookKind,
  type NamingHookTestCase,
  type NamingHookTestReport,
  type NamingHookTestResult,
  type PhotoFilenameFormatSettings,
  type PhotoNameField,
} from "../../api/settings";
import { selectDatabaseDestination, selectPhotoDirectory } from "../../api/dialogs";
import { Button, IconButton, SectionHeader } from "../../shared/ui";
import { CodeEditor } from "../../shared/CodeEditor";
import { ResizablePanels } from "../../shared/ResizablePanels";
import { SqlImportSettings } from "../taxonomy/SqlImportSettings";
import { DirectImportSettings } from "../taxonomy/DirectImportSettings";
import { AboutSettings } from "./AboutSettings";
import { PhotoLibrariesSettings } from "./PhotoLibrariesSettings";
import { emitMetadataChange } from "../../shared/metadataChanges";
import {
  defaultPhotoFilenameFormatSettings,
  normalizePhotoFilenameFormatSettings,
  photoFilenameFormatChanged,
  photoFilenameFormatFields,
  photoNamePriorityChanged,
  photoNamePriorityFields,
  photoNamePriorityLabels,
} from "./namingSettings";
import {
  canPresentHookResult,
  hookDraftMatchesSnapshot,
  type NamingHookSnapshot,
  replaceTestedHookSnapshot,
} from "./hookAsyncState";

export type SettingsSection = WorkspaceSettingsSection;

const settingsSections: Array<{
  id: SettingsSection;
  icon: LucideIcon;
}> = [
  { id: "General", icon: SlidersHorizontal },
  { id: "Storage", icon: FolderCog },
  { id: "Photo Libraries", icon: Library },
  { id: "Taxonomy Databases", icon: Database },
  { id: "Naming", icon: CaseSensitive },
  { id: "Map", icon: Map },
];

export function SettingsView({
  onTaxonomyImported,
  onSectionChange,
  onWorkspaceChanged,
  onOpenPhotoLibrary,
  onPhotoOperationStarted,
  photoLibraryOperation,
  photoLibraryOperationError,
  generalSettings,
  generalSettingsLoadError,
  onGeneralSettingsChange,
  section,
  taskOwnerId,
  onStatus,
}: {
  generalSettings: GeneralSettingsValue;
  generalSettingsLoadError?: string;
  onGeneralSettingsChange: (settings: GeneralSettingsValue) => void;
  onTaxonomyImported?: () => void;
  onSectionChange: (section: SettingsSection) => void;
  onWorkspaceChanged?: (resetPhotoTabs: boolean) => Promise<void>;
  onOpenPhotoLibrary: () => Promise<boolean>;
  onPhotoOperationStarted: (operation: OperationState | null) => void;
  photoLibraryOperation: OperationState | null;
  photoLibraryOperationError: string;
  section: SettingsSection;
  taskOwnerId: string;
  onStatus: (message: string) => void;
}) {
  const hookSection = section === "Filename Parser" || section === "Synonym Splitter";
  const taxonomyDatabaseSection = section === "Taxonomy Databases"
    || section === "SQL Import"
    || section === "Direct Import";
  const sqlImportSection = section === "Taxonomy Databases" || section === "SQL Import";
  const directImportSection = section === "Direct Import";
  const [hooksExpanded, setHooksExpanded] = useState(hookSection);
  const [taxonomyDatabasesExpanded, setTaxonomyDatabasesExpanded] = useState(taxonomyDatabaseSection);
  const [sqlImportMounted, setSqlImportMounted] = useState(sqlImportSection);
  const [directImportMounted, setDirectImportMounted] = useState(directImportSection);

  useEffect(() => {
    if (hookSection) setHooksExpanded(true);
  }, [hookSection]);

  useEffect(() => {
    if (taxonomyDatabaseSection) setTaxonomyDatabasesExpanded(true);
  }, [taxonomyDatabaseSection]);

  useEffect(() => {
    if (sqlImportSection) setSqlImportMounted(true);
  }, [sqlImportSection]);

  useEffect(() => {
    if (directImportSection) setDirectImportMounted(true);
  }, [directImportSection]);

  const shouldMountSqlImport = sqlImportMounted || sqlImportSection;
  const shouldMountDirectImport = directImportMounted || directImportSection;

  return (
    <ResizablePanels
      className="settings-workbench"
      initialSize={220}
      minFirst={210}
      minSecond={420}
      separatorLabel="Resize Settings navigation"
      stateKey="settings.navigation"
      first={(<aside className="settings-nav">
        {settingsSections.map(({ id, icon: Icon }) => id === "Taxonomy Databases" ? (
          <Fragment key={id}>
            <button className={taxonomyDatabaseSection ? "active" : ""} type="button" onClick={() => setTaxonomyDatabasesExpanded((current) => !current)}>
              <Icon size={14} />{id}
              {taxonomyDatabasesExpanded ? <ChevronDown className="settings-nav-chevron" size={13} /> : <ChevronRight className="settings-nav-chevron" size={13} />}
            </button>
            {taxonomyDatabasesExpanded && (
              <div className="settings-subnav">
                {(["SQL Import", "Direct Import"] as const).map((id) => {
                  const active = section === id || (id === "SQL Import" && section === "Taxonomy Databases");
                  return (
                    <button className={active ? "active" : ""} type="button" key={id} onClick={() => onSectionChange(id)}>
                      {id}
                    </button>
                  );
                })}
              </div>
            )}
          </Fragment>
        ) : (
          <button className={section === id ? "active" : ""} type="button" key={id} onClick={() => onSectionChange(id)}>
            <Icon size={14} />{id}
          </button>
        ))}
        <button className={hookSection ? "active" : ""} type="button" onClick={() => setHooksExpanded((current) => !current)}>
          <Plug size={14} />Hooks
          {hooksExpanded ? <ChevronDown className="settings-nav-chevron" size={13} /> : <ChevronRight className="settings-nav-chevron" size={13} />}
        </button>
        {hooksExpanded && (
          <div className="settings-subnav">
            {(["Filename Parser", "Synonym Splitter"] as const).map((id) => (
              <button className={section === id ? "active" : ""} type="button" key={id} onClick={() => onSectionChange(id)}>
                {id}<span className="settings-language-badge">Rhai</span>
              </button>
            ))}
          </div>
        )}
        <button className={section === "About" ? "active" : ""} type="button" onClick={() => onSectionChange("About")}>
          <Info size={14} />About
        </button>
      </aside>)}
      second={(<main className="settings-content">
        {section === "General" && (
          <GeneralSettings
            settings={generalSettings}
            loadError={generalSettingsLoadError}
            onChange={onGeneralSettingsChange}
            onStatus={onStatus}
          />
        )}
        {section === "Storage" && <StorageSettings onStatus={onStatus} />}
        {section === "Photo Libraries" && (
          <PhotoLibrariesSettings
            onChanged={onWorkspaceChanged}
            onOpenPhotoLibrary={onOpenPhotoLibrary}
            onPhotoOperationStarted={onPhotoOperationStarted}
            blockingOperation={photoLibraryOperation}
            operationError={photoLibraryOperationError}
          />
        )}
        {shouldMountSqlImport && <SqlImportSettings active={sqlImportSection} onApplied={onTaxonomyImported} taskOwnerId={taskOwnerId} />}
        {shouldMountDirectImport && <DirectImportSettings active={directImportSection} onApplied={onTaxonomyImported} taskOwnerId={taskOwnerId} />}
        {section === "Naming" && <NamingSettings onStatus={onStatus} />}
        {section === "Map" && <MapSettingsPanel onStatus={onStatus} />}
        {hookSection && <HooksSettings kind={section === "Filename Parser" ? "photo_filename" : "synonym_authority"} onStatus={onStatus} />}
        {section === "About" && <AboutSettings />}
      </main>)}
    />
  );
}

function GeneralSettings({
  settings,
  loadError = "",
  onChange,
  onStatus,
}: {
  settings: GeneralSettingsValue;
  loadError?: string;
  onChange: (settings: GeneralSettingsValue) => void;
  onStatus: (message: string) => void;
}) {
  const [saveError, setSaveError] = useState("");
  const [recentDraft, setRecentDraft] = useState(String(settings.recent_searches_limit));
  const [recentError, setRecentError] = useState("");
  const [saving, setSaving] = useState(false);
  const saveSequence = useRef(0);

  useEffect(() => {
    setRecentDraft(String(settings.recent_searches_limit));
  }, [settings.recent_searches_limit]);

  async function change(next: GeneralSettingsValue) {
    const previous = settings;
    const sequence = ++saveSequence.current;
    onChange(next);
    setSaving(true);
    setSaveError("");
    onStatus("Saving settings...");
    try {
      const saved = await updateGeneralSettings(next);
      if (sequence === saveSequence.current) {
        onChange(saved);
        if (previous.csv_delimiter !== saved.csv_delimiter) {
          emitMetadataChange({ key: "csv_delimiter", value: saved.csv_delimiter });
        }
        onStatus("Settings saved.");
      }
    } catch (nextError) {
      if (sequence === saveSequence.current) {
        onChange(previous);
        setSaveError(errorMessage(nextError));
        onStatus("Settings save failed.");
      }
    } finally {
      if (sequence === saveSequence.current) setSaving(false);
    }
  }

  function saveRecentSearchLimit() {
    if (saving) return;
    const value = Number(recentDraft);
    if (!/^\d+$/.test(recentDraft) || !Number.isInteger(value) || value < 1 || value > 50) {
      setRecentError("Recent searches limit must be an integer from 1 to 50.");
      return;
    }
    setRecentError("");
    if (value !== settings.recent_searches_limit) {
      void change({ ...settings, recent_searches_limit: value });
    }
  }

  function changeTaxonNamePart(
    field: "photos_taxon_name_parts" | "taxonomy_taxon_name_parts",
    key: keyof GeneralSettingsValue["photos_taxon_name_parts"],
    checked: boolean,
  ) {
    const next = {
      ...settings[field],
      [key]: checked,
    };
    if (!next.sci_name && !next.zh_name && !next.en_name) return;
    void change({ ...settings, [field]: next });
  }

  return (
    <div className="settings-section">
      <SectionHeader title="General" detail="Configure application-wide settings." />
      <section className="settings-group" aria-labelledby="general-appearance-heading">
        <h3 id="general-appearance-heading">Appearance</h3>
        <label className="general-setting-row">
          <span><strong>Theme</strong><small>Choose the application color scheme.</small></span>
          <select
            aria-label="Theme"
            disabled={saving}
            value={settings.theme}
            onChange={(event) => void change({
              ...settings,
              theme: event.target.value as GeneralSettingsValue["theme"],
            })}
          >
            <option value="system">System</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </label>
      </section>
      <section className="settings-group" aria-labelledby="general-startup-heading">
        <h3 id="general-startup-heading">Startup</h3>
        <label className="general-setting-row">
          <span><strong>Restore previously opened tabs</strong><small>Restore valid tabs and the active tab on the next launch.</small></span>
          <input
            aria-label="Restore previously opened tabs"
            disabled={saving}
            type="checkbox"
            checked={settings.restore_tabs}
            onChange={(event) => void change({ ...settings, restore_tabs: event.target.checked })}
          />
        </label>
      </section>
      <section className="settings-group" aria-labelledby="general-search-heading">
        <h3 id="general-search-heading">Search</h3>
        <label className="general-setting-row">
          <span><strong>Recent searches limit</strong><small>Keep between 1 and 50 searches on this computer.</small></span>
          <input
            aria-label="Recent searches limit"
            disabled={saving}
            type="number"
            min={1}
            max={50}
            value={recentDraft}
            aria-invalid={Boolean(recentError)}
            onChange={(event) => setRecentDraft(event.target.value)}
            onBlur={saveRecentSearchLimit}
            onKeyDown={(event) => {
              if (event.key === "Enter") saveRecentSearchLimit();
            }}
          />
        </label>
        {recentError && <div className="inline-error" role="alert">{recentError}</div>}
      </section>
      <section className="settings-group" aria-labelledby="general-csv-heading">
        <h3 id="general-csv-heading">CSV</h3>
        <label className="general-setting-row">
          <span><strong>CSV delimiter</strong><small>Used for every CSV import, export, and formatted-update template.</small></span>
          <select
            aria-label="CSV delimiter"
            disabled={saving}
            value={settings.csv_delimiter}
            onChange={(event) => void change({
              ...settings,
              csv_delimiter: event.target.value as GeneralSettingsValue["csv_delimiter"],
            })}
          >
            <option value=",">,</option>
            <option value=";">;</option>
            <option value={"\t"}>Tab (\t)</option>
            <option value="|">|</option>
          </select>
        </label>
      </section>
      <section className="settings-group" aria-labelledby="general-taxon-names-heading">
        <h3 id="general-taxon-names-heading">Taxon names</h3>
        {([
          ["photos_taxon_name_parts", "Photos", "Choose which accepted names are shown while browsing photos."],
          ["taxonomy_taxon_name_parts", "Taxonomy", "Choose which accepted names are shown in taxonomy navigation."],
        ] as const).map(([field, label, detail]) => {
          const visibleCount = Object.values(settings[field]).filter(Boolean).length;
          return (
            <div className="general-setting-row" key={field}>
              <span><strong>{label}</strong><small>{detail}</small></span>
              <div className="general-checkbox-row" role="group" aria-label={`${label} visible taxon names`}>
                {([
                  ["sci_name", "Scientific"],
                  ["zh_name", "Chinese"],
                  ["en_name", "English"],
                ] as const).map(([key, nameLabel]) => {
                  const checked = settings[field][key];
                  return (
                    <label key={key}>
                      <input
                        type="checkbox"
                        disabled={saving || (checked && visibleCount === 1)}
                        checked={checked}
                        onChange={(event) => changeTaxonNamePart(field, key, event.target.checked)}
                      />
                      <span>{nameLabel}</span>
                    </label>
                  );
                })}
              </div>
            </div>
          );
        })}
      </section>
      {(loadError || saveError) && <div className="inline-error" role="alert">{loadError || saveError}</div>}
    </div>
  );
}

function StorageSettings({ onStatus }: { onStatus: (message: string) => void }) {
  const [locations, setLocations] = useState<DatabaseLocations | null>(null);
  const [importMetadata, setImportMetadata] = useState<TaxonomyImportMetadata | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    void Promise.all([getDatabaseLocations(), getTaxonomyImportMetadata()])
      .then(([nextLocations, nextMetadata]) => {
        setLocations(nextLocations);
        setImportMetadata(nextMetadata);
      })
      .catch((nextError) => setError(errorMessage(nextError)));
  }, []);

  async function changeDefaultPhotoLibraryDirectory() {
    const directory = await selectPhotoDirectory(locations?.default_photo_library_directory);
    if (!directory) return;
    try {
      setLocations(await setDefaultPhotoLibraryDatabaseDirectory(directory));
      setError("");
      onStatus("Default storage directory updated.");
    } catch (nextError) {
      setError(errorMessage(nextError));
    }
  }

  async function relocate() {
    const destination = await selectDatabaseDestination(locations?.default_taxonomy_directory);
    if (!destination) return;
    try {
      setLocations(await relocateTaxonomyDatabase(destination));
      setError("");
      onStatus("Taxonomy Database relocated.");
    } catch (nextError) {
      setError(errorMessage(nextError));
    }
  }

  async function changeDefaultDirectory() {
    const directory = await selectPhotoDirectory(locations?.default_taxonomy_directory);
    if (!directory) return;
    try {
      setLocations(await setDefaultTaxonomyDatabaseDirectory(directory));
      setError("");
      onStatus("Default Taxonomy Database directory updated.");
    } catch (nextError) {
      setError(errorMessage(nextError));
    }
  }

  async function openStoragePath(path: string) {
    setError("");
    try {
      await openPathInFileManager(path);
    } catch (nextError) {
      setError(errorMessage(nextError));
    }
  }

  return (
    <div className="settings-section">
      <SectionHeader title="Storage" detail="View database locations, default directories, and taxonomy metadata." />
      <StoragePath
        label="Metadata database"
        value={locations?.metadata_database ?? "Loading"}
        onOpen={locations ? () => void openStoragePath(locations.metadata_database) : undefined}
      />
      <StoragePath
        label="Taxonomy database"
        value={locations?.taxonomy_database ?? "Loading"}
        onOpen={locations ? () => void openStoragePath(locations.taxonomy_database) : undefined}
        action={<Button onClick={() => void relocate()}><FolderInput size={13} />Move</Button>}
      />
      <StoragePath
        label="Photo Library database default directory"
        value={locations?.default_photo_library_directory ?? "Loading"}
        onOpen={locations ? () => void openStoragePath(locations.default_photo_library_directory) : undefined}
        action={<Button onClick={() => void changeDefaultPhotoLibraryDirectory()}><FolderCog size={13} />Change</Button>}
      />
      <StoragePath
        label="Taxonomy database default directory"
        value={locations?.default_taxonomy_directory ?? "Loading"}
        onOpen={locations ? () => void openStoragePath(locations.default_taxonomy_directory) : undefined}
        action={<Button onClick={() => void changeDefaultDirectory()}><FolderCog size={13} />Change</Button>}
      />
      <h3>Taxonomy metadata</h3>
      <StoragePath label="Current source" value={importMetadata?.source_path ?? "Not imported"} />
      <Setting label="Taxa" value={importMetadata ? String(importMetadata.taxa_count) : "-"} />
      <Setting label="Names" value={importMetadata ? String(importMetadata.taxon_names_count) : "-"} />
      <Setting label="Imported" value={importMetadata?.imported_at ?? "-"} />
      {error && <div className="inline-error" role="alert">{error}</div>}
    </div>
  );
}

function StoragePath({
  label,
  value,
  onOpen,
  action,
}: {
  label: string;
  value: string;
  onOpen?: () => void;
  action?: ReactNode;
}) {
  return <div className="storage-path"><span>{label}</span><code>{value}</code>{onOpen || action ? <div className="storage-actions">{onOpen ? <Button onClick={onOpen}><FolderOpen size={13} />Open</Button> : null}{action}</div> : null}</div>;
}

function NamingSettings({ onStatus }: { onStatus: (message: string) => void }) {
  const [priority, setPriority] = useState<PhotoNameField[]>([...photoNamePriorityFields]);
  const [format, setFormat] = useState<PhotoFilenameFormatSettings>(defaultPhotoFilenameFormatSettings);
  const [separator, setSeparator] = useState(";");
  const [loadError, setLoadError] = useState("");
  const [saveError, setSaveError] = useState("");
  const [separatorError, setSeparatorError] = useState("");
  const [saving, setSaving] = useState(false);
  const saveSequence = useRef(0);
  const savedPriority = useRef<PhotoNameField[]>([...photoNamePriorityFields]);
  const savedFormat = useRef<PhotoFilenameFormatSettings>(defaultPhotoFilenameFormatSettings());
  const savedSeparator = useRef(";");
  useEffect(() => {
    let active = true;
    void Promise.allSettled([
      getPhotoNameMatchSettings(),
      getPhotoFilenameFormatSettings(),
      getTaxonomyNameSeparator(),
    ]).then(([matchResult, formatResult, separatorResult]) => {
      if (!active) return;
      const errors: string[] = [];
      if (matchResult.status === "fulfilled") {
        setPriority(matchResult.value.priority);
        savedPriority.current = [...matchResult.value.priority];
      }
      else errors.push(`Mapping priority: ${errorMessage(matchResult.reason)}`);
      if (formatResult.status === "fulfilled") {
        const loadedFormat = normalizePhotoFilenameFormatSettings(formatResult.value);
        setFormat(loadedFormat);
        savedFormat.current = loadedFormat;
      } else {
        errors.push(`Photo filename format: ${errorMessage(formatResult.reason)}. Showing defaults.`);
      }
      if (separatorResult.status === "fulfilled") {
        setSeparator(separatorResult.value);
        savedSeparator.current = separatorResult.value;
      }
      else errors.push(`Multiple-name separator: ${errorMessage(separatorResult.reason)}`);
      setLoadError(errors.join(" "));
    });
    return () => { active = false; };
  }, []);

  function move(index: number, direction: -1 | 1) {
    const target = index + direction;
    if (saving || target < 0 || target >= priority.length) return;
    const next = [...priority];
    [next[index], next[target]] = [next[target], next[index]];
    setPriority(next);
    void savePriority(next);
  }

  async function savePriority(next: PhotoNameField[]) {
    if (!photoNamePriorityChanged(savedPriority.current, next)) return;
    const previous = [...savedPriority.current];
    const sequence = ++saveSequence.current;
    setSaving(true);
    setSaveError("");
    onStatus("Saving naming settings...");
    try {
      await setPhotoNameMatchSettings({ priority: next });
      if (sequence === saveSequence.current) {
        savedPriority.current = [...next];
        onStatus("Naming settings saved.");
      }
    } catch (nextError) {
      if (sequence === saveSequence.current) {
        setPriority(previous);
        setSaveError(errorMessage(nextError));
        onStatus("Naming settings save failed.");
      }
    } finally {
      if (sequence === saveSequence.current) setSaving(false);
    }
  }

  async function saveFormat(next: PhotoFilenameFormatSettings) {
    if (!photoFilenameFormatChanged(savedFormat.current, next)) return;
    const previous = { ...savedFormat.current };
    const sequence = ++saveSequence.current;
    setFormat(next);
    setSaving(true);
    setSaveError("");
    onStatus("Saving naming settings...");
    try {
      await setPhotoFilenameFormatSettings(next);
      if (sequence === saveSequence.current) {
        savedFormat.current = { ...next };
        onStatus("Naming settings saved.");
      }
    } catch (nextError) {
      if (sequence === saveSequence.current) {
        setFormat(previous);
        setSaveError(errorMessage(nextError));
        onStatus("Naming settings save failed.");
      }
    } finally {
      if (sequence === saveSequence.current) setSaving(false);
    }
  }

  async function saveSeparator() {
    if (saving) return;
    if (separator.length !== 1) {
      setSeparatorError("Multiple-name separator must be one character.");
      return;
    }
    setSeparatorError("");
    if (separator === savedSeparator.current) return;
    const previous = savedSeparator.current;
    const sequence = ++saveSequence.current;
    setSaving(true);
    setSaveError("");
    onStatus("Saving naming settings...");
    try {
      await setTaxonomyNameSeparator(separator);
      if (sequence === saveSequence.current) {
        savedSeparator.current = separator;
        emitMetadataChange({ key: "taxonomy_name_separator", value: separator });
        onStatus("Naming settings saved.");
      }
    } catch (nextError) {
      if (sequence === saveSequence.current) {
        setSeparator(previous);
        setSaveError(errorMessage(nextError));
        onStatus("Naming settings save failed.");
      }
    } finally {
      if (sequence === saveSequence.current) setSaving(false);
    }
  }

  return (
    <div className="settings-section">
      <SectionHeader title="Naming" detail="Configure taxonomy matching and mapped-photo filename generation." />
      <div className="field-stack">
        <span><strong>Mapping name priority</strong></span>
        <small>Within each field, accepted names are matched first. Aliases or synonyms are used only when no accepted-name match exists.</small>
        <div className="priority-list">
          {priority.map((field, index) => (
            <div key={field}>
              <b>{index + 1}</b><span>{photoNamePriorityLabels[field]}</span>
              <IconButton
                aria-label={`Move ${photoNamePriorityLabels[field]} up`}
                disabled={saving || index === 0}
                onClick={() => move(index, -1)}
              ><ArrowUp size={13} /></IconButton>
              <IconButton
                aria-label={`Move ${photoNamePriorityLabels[field]} down`}
                disabled={saving || index === priority.length - 1}
                onClick={() => move(index, 1)}
              ><ArrowDown size={13} /></IconButton>
            </div>
          ))}
        </div>
      </div>
      <div className="field-stack">
        <span><strong>Photo filename format</strong></span>
        <div className="checkbox-grid">{photoFilenameFormatFields.map(({ field, label }) => (
          <label key={field}><input type="checkbox" disabled={saving || (format[field] && Object.values(format).filter(Boolean).length === 1)} checked={format[field]} onChange={(event) => void saveFormat({ ...format, [field]: event.target.checked })} />{label}</label>
        ))}</div>
      </div>
      <label className="field-stack"><span><strong>Multiple-name separator</strong></span><input value={separator} maxLength={1} disabled={saving} aria-invalid={Boolean(separatorError)} onChange={(event) => setSeparator(event.target.value)} onBlur={() => void saveSeparator()} onKeyDown={(event) => { if (event.key === "Enter") void saveSeparator(); }} /></label>
      {separatorError && <div className="inline-error" role="alert">{separatorError}</div>}
      {(loadError || saveError) && <div className="inline-error" role="alert">{loadError || saveError}</div>}
    </div>
  );
}

function MapSettingsPanel({ onStatus }: { onStatus: (message: string) => void }) {
  const [settings, setSettings] = useState<MapSettings>({ provider: "osm", tianditu_token: null });
  const [tokenDraft, setTokenDraft] = useState("");
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const saveSequence = useRef(0);
  useEffect(() => {
    void getMapSettings().then((loaded) => {
      setSettings(loaded);
      setTokenDraft(loaded.tianditu_token ?? "");
    }).catch((nextError) => setError(errorMessage(nextError)));
  }, []);

  async function save(next: MapSettings) {
    const previous = settings;
    const sequence = ++saveSequence.current;
    setSettings(next);
    setSaving(true);
    setError("");
    onStatus("Saving map settings...");
    try {
      const saved = await setMapSettings(next);
      if (sequence === saveSequence.current) {
        setSettings(saved);
        setTokenDraft(saved.tianditu_token ?? "");
        onStatus("Map settings saved.");
      }
    } catch (nextError) {
      if (sequence === saveSequence.current) {
        setSettings(previous);
        setTokenDraft(previous.tianditu_token ?? "");
        setError(errorMessage(nextError));
        onStatus("Map settings save failed.");
      }
    } finally {
      if (sequence === saveSequence.current) setSaving(false);
    }
  }

  function saveToken() {
    if (saving) return;
    const token = tokenDraft || null;
    if (token !== settings.tianditu_token) void save({ ...settings, tianditu_token: token });
  }

  return (
    <div className="settings-section">
      <SectionHeader title="Map" detail="Configure the map tile provider and provider credentials." />
      <label className="field-stack"><span><strong>Tile provider</strong></span><select disabled={saving} value={settings.provider} onChange={(event) => void save({ ...settings, provider: event.target.value as MapSettings["provider"] })}><option value="osm">OpenStreetMap</option><option value="tianditu">Tianditu</option></select></label>
      <label className="field-stack"><span><strong>Token</strong></span><input disabled={saving || settings.provider !== "tianditu"} value={tokenDraft} onChange={(event) => setTokenDraft(event.target.value)} onBlur={saveToken} onKeyDown={(event) => { if (event.key === "Enter") saveToken(); }} /></label>
      {error && <div className="inline-error" role="alert">{error}</div>}
    </div>
  );
}

function HooksSettings({ kind, onStatus }: { kind: NamingHookKind; onStatus: (message: string) => void }) {
  const [scripts, setScripts] = useState<Record<NamingHookKind, string>>({ photo_filename: "", synonym_authority: "" });
  const [cases, setCases] = useState<Record<NamingHookKind, NamingHookTestCase[]>>({ photo_filename: [], synonym_authority: [] });
  const [report, setReport] = useState<NamingHookTestReport | null>(null);
  const [testedSnapshots, setTestedSnapshots] = useState<Record<NamingHookKind, NamingHookSnapshot | null>>({
    photo_filename: null,
    synonym_authority: null,
  });
  const [busy, setBusy] = useState("");
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const draftSequence = useRef<Record<NamingHookKind, number>>({ photo_filename: 0, synonym_authority: 0 });
  const activeKindRef = useRef(kind);

  useEffect(() => {
    let active = true;
    getNamingHookTemplates().then((nextTemplates) => {
      if (!active) return;
      setScripts(nextTemplates);
      return getNamingHookSettings().then((settings) => {
        if (!active) return;
        setScripts({
          photo_filename: settings.photo_filename?.trim() ? settings.photo_filename : nextTemplates.photo_filename,
          synonym_authority: settings.synonym_authority?.trim() ? settings.synonym_authority : nextTemplates.synonym_authority,
        });
      });
    }).catch((nextError) => {
      if (active) setError(errorMessage(nextError));
    });
    getNamingHookTestCases().then((nextCases) => {
      if (active) setCases(nextCases);
    }).catch((nextError) => {
      if (active) setError(errorMessage(nextError));
    });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    activeKindRef.current = kind;
    setReport(null);
    setMessage("");
    setError("");
  }, [kind]);

  function invalidate(nextScripts = scripts, nextCases = cases) {
    setScripts(nextScripts);
    setCases(nextCases);
    setTestedSnapshots((current) => replaceTestedHookSnapshot(current, kind, null));
    setReport(null);
    draftSequence.current[kind] += 1;
  }

  async function run() {
    const testedKind = kind;
    const testedRevision = draftSequence.current[testedKind];
    const snapshot = {
      script: scripts[testedKind],
      cases: cases[testedKind],
    };
    setTestedSnapshots((current) => replaceTestedHookSnapshot(current, testedKind, null));
    setBusy("Testing Hook");
    setError("");
    setMessage("");
    onStatus("Testing Hook and project tests...");
    try {
      const next = await runNamingHookTests(testedKind, snapshot.script, snapshot.cases);
      const canPresent = canPresentHookResult(
        testedKind,
        activeKindRef.current,
        testedRevision,
        draftSequence.current[testedKind],
      );
      if (canPresent) setReport(next);
      if (next.failed === 0) {
        if (canPresent) {
          setTestedSnapshots((current) => replaceTestedHookSnapshot(current, testedKind, snapshot));
          setMessage("All tests passed.");
          onStatus("All tests passed.");
        }
      } else {
        if (canPresent) setMessage("Tests failed. Review the actual output.");
        onStatus("Hook tests failed.");
      }
    } catch (nextError) {
      if (activeKindRef.current === testedKind) setError(errorMessage(nextError));
      onStatus("Hook operation failed.");
    } finally {
      setBusy("");
    }
  }

  async function save() {
    const testedKind = kind;
    const snapshot = testedSnapshots[testedKind];
    if (!snapshot || busy || !hookDraftMatchesSnapshot(scripts[testedKind], cases[testedKind], snapshot)) return;
    setBusy("Saving Hook");
    setError("");
    onStatus("Saving Hook and project tests...");
    try {
      await saveNamingHook(testedKind, snapshot.script, snapshot.cases);
      if (activeKindRef.current === testedKind) {
        onStatus("Hook and project tests saved.");
      }
    } catch (nextError) {
      if (activeKindRef.current === testedKind) setError(errorMessage(nextError));
      onStatus("Hook operation failed.");
    } finally {
      setBusy("");
    }
  }

  const saveAvailable = hookDraftMatchesSnapshot(scripts[kind], cases[kind], testedSnapshots[kind]);

  const title = kind === "photo_filename" ? "Filename Parser" : "Synonym Splitter";
  const detail = kind === "photo_filename"
    ? "Edit and test the hook that extracts taxonomy information from filenames."
    : "Edit and test the hook that separates names from authority information.";

  return (
    <div className="hooks-settings">
      <SectionHeader title={title} detail={detail} actions={(
        <>
          <Button disabled={Boolean(busy) || !scripts[kind].trim()} onClick={() => void run()}><BugPlay size={13} />{busy === "Testing Hook" ? "Testing..." : "Test"}</Button>
          <Button variant="primary" disabled={Boolean(busy) || !saveAvailable} onClick={() => void save()}><Save size={13} />{busy === "Saving Hook" ? "Saving..." : "Save"}</Button>
        </>
      )} />
      <ResizablePanels
        className="hook-columns"
        initialRatio={{ horizontal: 0.55, vertical: 0.52 }}
        minFirst={{ horizontal: 320, vertical: 180 }}
        minSecond={{ horizontal: 300, vertical: 180 }}
        responsiveBreakpoint={800}
        separatorLabel="Resize Hook editor and Project tests"
        stateKey={`settings.hooks.${kind}`}
        first={(<CodeEditor
            language="rhai"
            ariaLabel={`${kind} Rhai source`}
            value={scripts[kind]}
            onChange={(value) => invalidate({ ...scripts, [kind]: value })}
          />)}
        second={(<div className="hook-tests">
          <header><strong>Project tests</strong><Button variant="ghost" size="small" onClick={() => invalidate(scripts, { ...cases, [kind]: [...cases[kind], { input: "", expected: { kind, output: {} } }] })}>+ Add</Button></header>
          <div className="hook-test-list">
            {cases[kind].map((item, index) => (
              <div className={`hook-test-row${report?.cases[index] && !report.cases[index].passed ? " failed" : ""}`} key={index}>
                <header>
                  <strong>Test {index + 1}</strong>
                  {report?.cases[index] && <span className={report.cases[index].passed ? "passed" : "failed"}>{report.cases[index].passed ? "Passed" : "Failed"}</span>}
                  <IconButton size="small" aria-label={`Delete Test ${index + 1}`} title={`Delete Test ${index + 1}`} onClick={() => invalidate(scripts, { ...cases, [kind]: cases[kind].filter((_, itemIndex) => itemIndex !== index) })}><Trash2 size={12} /></IconButton>
                </header>
                <label className="hook-raw-field"><span>Raw input</span><input className="hook-raw-input" value={item.input} onChange={(event) => invalidate(scripts, changeCase(cases, kind, index, { ...item, input: event.target.value }))} /></label>
                <label><span>Expected output</span>
                  <ExpectedEditor
                    value={item.expected}
                    testNumber={index + 1}
                    onDraftChange={() => invalidate()}
                    onChange={(expected) => invalidate(scripts, changeCase(cases, kind, index, { ...item, expected }))}
                  />
                </label>
                {report?.cases[index] && (
                  <div className="hook-test-actual">
                    <span>Actual output</span>
                    <CodeEditor
                      autoGrow
                      language="json"
                      ariaLabel={`Test ${index + 1} actual output`}
                      minHeight={72}
                      maxHeight={160}
                      onChange={() => undefined}
                      readOnly
                      value={JSON.stringify(report.cases[index].actual, null, 2)}
                    />
                    {report.cases[index].error && <p>{report.cases[index].error}</p>}
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>)}
      />
      {report && <div className="editor-message">{report.passed} passed, {report.failed} failed. {message}</div>}
      {error && <div className="inline-error" role="alert">{error}</div>}
    </div>
  );
}

function ExpectedEditor({
  value,
  testNumber,
  onDraftChange,
  onChange,
}: {
  value: NamingHookTestResult;
  testNumber: number;
  onDraftChange: () => void;
  onChange: (value: NamingHookTestResult) => void;
}) {
  const serialized = JSON.stringify(value, null, 2);
  const [draft, setDraft] = useState(serialized);
  const [error, setError] = useState("");

  useEffect(() => setDraft(serialized), [serialized]);

  function update(next: string) {
    setDraft(next);
    onDraftChange();
    try {
      const parsed = JSON.parse(next) as NamingHookTestResult;
      if (!parsed || typeof parsed !== "object" || !("kind" in parsed) || !("output" in parsed)) {
        throw new Error("Expected a tagged hook result");
      }
      setError("");
      onChange(parsed);
    } catch {
      setError("Invalid expected JSON");
    }
  }

  return (
    <div className={`hook-expected${error ? " invalid" : ""}`} title={error}>
      <CodeEditor
        autoGrow
        language="json"
        ariaLabel={`Test ${testNumber} expected output`}
        minHeight={72}
        maxHeight={160}
        value={draft}
        onChange={update}
      />
    </div>
  );
}

function changeCase(
  cases: Record<NamingHookKind, NamingHookTestCase[]>,
  kind: NamingHookKind,
  index: number,
  value: NamingHookTestCase,
): Record<NamingHookKind, NamingHookTestCase[]> {
  return { ...cases, [kind]: cases[kind].map((item, itemIndex) => itemIndex === index ? value : item) };
}

function Setting({ label, value }: { label: string; value: string }) {
  return <div className="setting-row"><span>{label}</span><strong>{value}</strong></div>;
}
