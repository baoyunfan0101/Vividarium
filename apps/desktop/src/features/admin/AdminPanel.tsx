import { useEffect, useState } from "react";
import { ArrowDown, ArrowUp, Download, FileSearch, FolderOpen, Minus, Plus } from "lucide-react";
import { downloadTable, getMappingMetadata, getOperationsStatus, getPhotoRootsMetadata, getTaxaMetadata, operationFromResponse, saveKnowledgeBasePath, savePhotoRoots, selectLocalDirectory, selectLocalFile, startMappingRebuild, startMappingUpdate, startPhotosRebuild, startPhotosUpdate, startTaxaRebuild, startTaxaUpdate, waitForOperation, type MappingMetadata, type OperationState, type TaxaMetadata } from "../../bridge";
import { AdminActionArea } from "../../components/status";
import { EXPORT_TABLES } from "./constants";
import type { ExportModule, RootRow } from "./types";
import { formatBytes, formatOperationAlert, isConfirmationResponse, mergeRootSelection, moveSelectedRows, operationLabel, uniqueRoots } from "./adminUtils";
import { readStorage, writeStorage } from "../../lib/storage";
import { displayPath } from "../../lib/pathUtils";
import { MAP_TILE_PROVIDERS, readMapSettings, writeMapSettings, type MapProviderId, type MapSettings } from "../map/mapProviders";

const ADMIN_ROOT_ROWS_KEY = "phytoindex.admin.rootRows";
const ADMIN_TAXA_METADATA_KEY = "phytoindex.admin.taxaMetadata";
const ADMIN_MAPPING_METADATA_KEY = "phytoindex.admin.mappingMetadata";
const ADMIN_EXPORT_KEY = "phytoindex.admin.export";
const ADMIN_OPERATIONS_KEY = "phytoindex.admin.operations";

export function AdminPanel({ setMessage }: { setMessage: (message: string) => void }) {
  const [rootRows, setRootRows] = useState<RootRow[]>(() => readStorage<RootRow[]>(ADMIN_ROOT_ROWS_KEY, []));
  const [taxaMetadata, setTaxaMetadata] = useState<TaxaMetadata | null>(() => readStorage<TaxaMetadata | null>(ADMIN_TAXA_METADATA_KEY, null));
  const [knowledgeBasePath, setKnowledgeBasePath] = useState(() => readStorage<TaxaMetadata | null>(ADMIN_TAXA_METADATA_KEY, null)?.knowledge_base_path ?? "");
  const [mappingMetadata, setMappingMetadata] = useState<MappingMetadata | null>(() => readStorage<MappingMetadata | null>(ADMIN_MAPPING_METADATA_KEY, null));
  const [operations, setOperations] = useState<Record<OperationState["module"], OperationState> | null>(() => readStorage<Record<OperationState["module"], OperationState> | null>(ADMIN_OPERATIONS_KEY, null));
  const cachedExport = readStorage<{ module: ExportModule; table: string }>(ADMIN_EXPORT_KEY, {
    module: "photos",
    table: EXPORT_TABLES.photos[0]
  });
  const [exportModule, setExportModule] = useState<ExportModule>(cachedExport.module);
  const [exportTable, setExportTable] = useState(cachedExport.table);
  const [mapSettings, setMapSettings] = useState<MapSettings>(readMapSettings);
  const [editingRootIndex, setEditingRootIndex] = useState<number | null>(null);
  const [localBusy, setLocalBusy] = useState({
    photos: false,
    taxa: false,
    mapping: false,
    export: false
  });

  useEffect(() => {
    getPhotoRootsMetadata()
      .then((metadata) => setRootRows((rows) => {
        const nextRows = mergeRootSelection(metadata, rows);
        writeStorage(ADMIN_ROOT_ROWS_KEY, nextRows);
        return nextRows;
      }))
      .catch((error) => setMessage(error.message));
    getTaxaMetadata()
      .then((metadata) => {
        setTaxaMetadata(metadata);
        setKnowledgeBasePath(metadata.knowledge_base_path ?? "");
        writeStorage(ADMIN_TAXA_METADATA_KEY, metadata);
      })
      .catch((error) => setMessage(error.message));
    getMappingMetadata()
      .then((metadata) => {
        setMappingMetadata(metadata);
        writeStorage(ADMIN_MAPPING_METADATA_KEY, metadata);
      })
      .catch((error) => setMessage(error.message));
    refreshOperations();
    const timer = window.setInterval(refreshOperations, 1000);
    window.addEventListener("focus", refreshOperations);
    document.addEventListener("visibilitychange", refreshOperations);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", refreshOperations);
      document.removeEventListener("visibilitychange", refreshOperations);
    };
  }, [setMessage]);

  async function execute(module: OperationState["module"], label: string, action: () => Promise<unknown>) {
    setLocalBusy((state) => ({ ...state, [module]: true }));
    try {
      const result = await action();
      const operation = operationFromResponse(result);
      if (operation) {
        setOperations((state) => {
          const nextState = { ...(state ?? emptyOperations()), [operation.module]: operation };
          writeStorage(ADMIN_OPERATIONS_KEY, nextState);
          return nextState;
        });
        const finalResult = await waitForOperation(operation);
        alert(formatOperationAlert(label, finalResult));
      } else if (isConfirmationResponse(result)) {
        alert(`${label} canceled.`);
      } else if (!isConfirmationResponse(result)) {
        alert(formatOperationAlert(label, result));
      }
      refreshAdminMetadata();
      refreshOperations();
    } catch (error) {
      alert(`${label} failed: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setLocalBusy((state) => ({ ...state, [module]: false }));
    }
  }

  function refreshOperations() {
    getOperationsStatus()
      .then((nextOperations) => {
        setOperations(nextOperations);
        writeStorage(ADMIN_OPERATIONS_KEY, nextOperations);
      })
      .catch((error) => setMessage(error.message));
  }

  function refreshAdminMetadata() {
    getPhotoRootsMetadata()
      .then((metadata) => setRootRows((rows) => {
        const nextRows = mergeRootSelection(metadata, rows);
        writeStorage(ADMIN_ROOT_ROWS_KEY, nextRows);
        return nextRows;
      }))
      .catch((error) => setMessage(error.message));
    getTaxaMetadata()
      .then((metadata) => {
        setTaxaMetadata(metadata);
        setKnowledgeBasePath(metadata.knowledge_base_path ?? "");
        writeStorage(ADMIN_TAXA_METADATA_KEY, metadata);
      })
      .catch((error) => setMessage(error.message));
    getMappingMetadata()
      .then((metadata) => {
        setMappingMetadata(metadata);
        writeStorage(ADMIN_MAPPING_METADATA_KEY, metadata);
      })
      .catch((error) => setMessage(error.message));
  }

  const exportTables = EXPORT_TABLES[exportModule];
  const filledRoots = rootRows.filter((row) => row.root.trim());
  const selectedRoots = rootRows
    .filter((row) => row.selected && row.root.trim())
    .map((row) => row.root.trim());
  const allRootsSelected = filledRoots.length > 0 && filledRoots.every((row) => row.selected);
  const photosActive = localBusy.photos || Boolean(operations?.photos.running);
  const taxaActive = localBusy.taxa || Boolean(operations?.taxa.running);
  const mappingActive = localBusy.mapping || Boolean(operations?.mapping.running);
  const photosBlocked = photosActive || mappingActive;
  const taxaBlocked = taxaActive || mappingActive;
  const mappingBlocked = photosActive || taxaActive || mappingActive;
  const exportActive = localBusy.export;

  async function persistRoots(nextRows: RootRow[]) {
    setRootRows(nextRows);
    try {
      const metadata = await savePhotoRoots(uniqueRoots(nextRows.map((row) => row.root.trim()).filter(Boolean)));
      const mergedRows = mergeRootSelection(metadata, nextRows);
      setRootRows(mergedRows);
      writeStorage(ADMIN_ROOT_ROWS_KEY, mergedRows);
      setMessage("Photo roots saved");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  }

  function addRoot() {
    const nextRows = [
      ...rootRows,
      {
        root: "",
        last_synced_at: null,
        photo_count: 0,
        sort_order: rootRows.length,
        selected: true
      }
    ];
    setRootRows(nextRows);
    writeStorage(ADMIN_ROOT_ROWS_KEY, nextRows);
  }

  function removeSelectedRoots() {
    persistRoots(rootRows.filter((row) => !row.selected));
  }

  function moveSelectedRoots(direction: -1 | 1) {
    const nextRows = moveSelectedRows(rootRows, direction);
    persistRoots(nextRows);
  }

  function updateRootValue(index: number, root: string) {
    const nextRows = rootRows.map((row, rowIndex) => rowIndex === index ? { ...row, root } : row);
    setRootRows(nextRows);
    writeStorage(ADMIN_ROOT_ROWS_KEY, nextRows);
  }

  function toggleRoot(index: number) {
    const nextRows = rootRows.map((row, rowIndex) => rowIndex === index ? { ...row, selected: !row.selected } : row);
    setRootRows(nextRows);
    writeStorage(ADMIN_ROOT_ROWS_KEY, nextRows);
  }

  async function browseRoot(index: number) {
    try {
      const path = await selectLocalDirectory();
      if (!path) {
        return;
      }
      const nextRows = rootRows.map((row, rowIndex) => rowIndex === index ? { ...row, root: path } : row);
      persistRoots(nextRows);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  }

  function updateSelectedRoots() {
    if (!selectedRoots.length) {
      setMessage("Select at least one root");
      return;
    }
    execute("photos", "Photos update", () => startPhotosUpdate(selectedRoots));
  }

  function rebuildSelectedRoots() {
    if (!allRootsSelected) {
      setMessage("Rebuild Photos requires selecting all roots");
      return;
    }
    execute("photos", "Photos rebuild", () => startPhotosRebuild(selectedRoots));
  }

  async function persistKnowledgeBasePath() {
    try {
      const metadata = await saveKnowledgeBasePath(knowledgeBasePath.trim() || null);
      setTaxaMetadata(metadata);
      setKnowledgeBasePath(metadata.knowledge_base_path ?? "");
      writeStorage(ADMIN_TAXA_METADATA_KEY, metadata);
      setMessage("Knowledge base path saved");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  }

  async function browseKnowledgeBasePath() {
    try {
      const path = await selectLocalFile();
      if (!path) {
        return;
      }
      const metadata = await saveKnowledgeBasePath(path);
      setTaxaMetadata(metadata);
      setKnowledgeBasePath(metadata.knowledge_base_path ?? "");
      writeStorage(ADMIN_TAXA_METADATA_KEY, metadata);
      setMessage("Knowledge base path saved");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  }

  function selectExportModule(moduleName: ExportModule) {
    const firstTable = EXPORT_TABLES[moduleName][0];
    setExportModule(moduleName);
    setExportTable(firstTable);
    writeStorage(ADMIN_EXPORT_KEY, { module: moduleName, table: firstTable });
  }

  function updateMapSettings(nextSettings: MapSettings) {
    setMapSettings(nextSettings);
    writeMapSettings(nextSettings);
    setMessage("Map settings saved");
  }

  async function exportCurrentTable() {
    setLocalBusy((state) => ({ ...state, export: true }));
    setMessage(`Exporting ${exportTable}`);
    try {
      const filename = await downloadTable(exportTable);
      setMessage(filename ? `Exported ${filename}` : "Export canceled");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setLocalBusy((state) => ({ ...state, export: false }));
    }
  }

  return (
    <section className="admin-grid">
      <div className="panel photos-admin">
        <div className="panel-heading">
          <div>
            <h2>Photos</h2>
            <p className="panel-subtitle">Photo roots</p>
          </div>
          <div className="icon-toolbar">
            <button type="button" disabled={photosBlocked} title="Add root" onClick={addRoot}><Plus size={16} /></button>
            <button type="button" disabled={photosBlocked} title="Remove selected roots" onClick={removeSelectedRoots}><Minus size={16} /></button>
            <button type="button" disabled={photosBlocked} title="Move selected roots up" onClick={() => moveSelectedRoots(-1)}><ArrowUp size={16} /></button>
            <button type="button" disabled={photosBlocked} title="Move selected roots down" onClick={() => moveSelectedRoots(1)}><ArrowDown size={16} /></button>
          </div>
        </div>
        <div className="root-table">
          {rootRows.map((row, index) => (
            <div className={row.selected ? "root-row selected" : "root-row"} key={`${row.sort_order}-${index}`}>
              <input
                type="checkbox"
                checked={row.selected}
                disabled={photosBlocked}
                onChange={() => toggleRoot(index)}
                aria-label="Select root"
              />
              <input
                value={editingRootIndex === index ? row.root : displayPath(row.root)}
                disabled={photosBlocked}
                onFocus={() => setEditingRootIndex(index)}
                onChange={(event) => updateRootValue(index, event.target.value)}
                onBlur={() => {
                  setEditingRootIndex(null);
                  persistRoots(rootRows);
                }}
                placeholder="/path/to/photos"
              />
              <button type="button" disabled={photosBlocked} title="Browse root" onClick={() => browseRoot(index)}><FolderOpen size={15} /></button>
              <span>{row.photo_count ?? 0} photos · Updated: {row.last_synced_at ?? "not synced"}</span>
            </div>
          ))}
        </div>
        <AdminActionArea active={photosActive} label={operationLabel(operations?.photos, "Photos")} processed={operations?.photos.processed} total={operations?.photos.total}>
          <button type="button" disabled={photosBlocked} onClick={updateSelectedRoots}>Update Photos</button>
          <button type="button" disabled={photosBlocked || !allRootsSelected} title={allRootsSelected ? "Rebuild Photos" : "Select all roots to rebuild photos"} onClick={rebuildSelectedRoots}>Rebuild Photos</button>
        </AdminActionArea>
      </div>
      <div className="panel">
        <h2>Taxa</h2>
        <p className="panel-subtitle">Knowledge base path</p>
        <div className="path-input-row">
          <input value={knowledgeBasePath} disabled={taxaBlocked} onChange={(event) => setKnowledgeBasePath(event.target.value)} onBlur={persistKnowledgeBasePath} placeholder="/path/to/plants.xlsm" />
          <button type="button" disabled={taxaBlocked} title="Browse knowledge base" onClick={browseKnowledgeBasePath}><FileSearch size={15} /></button>
        </div>
        <div className="metadata-list">
          <div><span>Last synced</span><strong>{taxaMetadata?.last_synced_at ?? "not synced"}</strong></div>
          <div><span>File modified</span><strong>{taxaMetadata?.knowledge_base_modified_at ?? "unknown"}</strong></div>
          <div><span>File size</span><strong>{formatBytes(taxaMetadata?.knowledge_base_size)}</strong></div>
          <div><span>Taxa rows</span><strong>{taxaMetadata?.taxa_count ?? 0}</strong></div>
        </div>
        <AdminActionArea active={taxaActive} label={operationLabel(operations?.taxa, "Taxa")} processed={operations?.taxa.processed} total={operations?.taxa.total}>
          <button type="button" disabled={taxaBlocked} onClick={() => execute("taxa", "Taxa update", () => startTaxaUpdate(knowledgeBasePath || null))}>Update Taxa</button>
          <button type="button" disabled={taxaBlocked} onClick={() => execute("taxa", "Taxa rebuild", () => startTaxaRebuild(knowledgeBasePath || null))}>Rebuild Taxa</button>
        </AdminActionArea>
      </div>
      <div className="panel">
        <h2>Mapping</h2>
        <p className="panel-subtitle">Sync metadata</p>
        <div className="metadata-list">
          <div><span>Last synced</span><strong>{mappingMetadata?.last_synced_at ?? "not synced"}</strong></div>
          <div><span>Photos synced</span><strong>{mappingMetadata?.photos_last_synced_at ?? "unknown"}</strong></div>
          <div><span>Taxa synced</span><strong>{mappingMetadata?.taxa_last_synced_at ?? "unknown"}</strong></div>
          <div><span>Mapped photos</span><strong>{mappingMetadata?.mapped_photo_count ?? 0}</strong></div>
          <div><span>Mapping taxa</span><strong>{mappingMetadata?.mapping_taxa_count ?? 0}</strong></div>
        </div>
        <AdminActionArea active={mappingActive} label={operationLabel(operations?.mapping, "Mapping")} processed={operations?.mapping.processed} total={operations?.mapping.total}>
          <button type="button" disabled={mappingBlocked} onClick={() => execute("mapping", "Mapping update", startMappingUpdate)}>Update Mapping</button>
          <button type="button" disabled={mappingBlocked} onClick={() => execute("mapping", "Mapping rebuild", startMappingRebuild)}>Rebuild Mapping</button>
        </AdminActionArea>
      </div>
      <div className="panel">
        <h2>Map</h2>
        <p className="panel-subtitle">Tile provider</p>
        <select value={mapSettings.providerId} onChange={(event) => updateMapSettings({ ...mapSettings, providerId: event.target.value as MapProviderId })}>
          {MAP_TILE_PROVIDERS.map((provider) => (
            <option value={provider.id} key={provider.id}>{provider.name}</option>
          ))}
        </select>
        {mapSettings.providerId === "tianditu" && (
          <label className="map-token-field">
            <span>Application token (tk)</span>
            <input
              type="password"
              value={mapSettings.tiandituToken}
              autoComplete="off"
              spellCheck={false}
              placeholder="Enter your Tianditu token"
              onChange={(event) => setMapSettings({ ...mapSettings, tiandituToken: event.target.value })}
              onBlur={() => updateMapSettings(mapSettings)}
            />
          </label>
        )}
        <div className="metadata-list">
          <div><span>Renderer</span><strong>MapLibre GL JS</strong></div>
          <div><span>Provider</span><strong>{MAP_TILE_PROVIDERS.find((provider) => provider.id === mapSettings.providerId)?.name ?? mapSettings.providerId}</strong></div>
          {mapSettings.providerId === "tianditu" && <div><span>Application token</span><strong>{mapSettings.tiandituToken ? "Configured" : "Required"}</strong></div>}
        </div>
      </div>
      <div className="panel">
        <h2>Export Tables</h2>
        <label>Module</label>
        <select value={exportModule} disabled={exportActive} onChange={(event) => selectExportModule(event.target.value as ExportModule)}>
          <option value="photos">Photos</option>
          <option value="taxa">Taxa</option>
          <option value="mapping">Photos-Taxa Mapping</option>
        </select>
        <label>Table</label>
        <select value={exportTable} disabled={exportActive} onChange={(event) => {
          setExportTable(event.target.value);
          writeStorage(ADMIN_EXPORT_KEY, { module: exportModule, table: event.target.value });
        }}>
          {exportTables.map((table) => <option value={table} key={table}>{table}</option>)}
        </select>
        <AdminActionArea active={exportActive} label={`Exporting ${exportTable}`}>
          <button type="button" disabled={exportActive} onClick={exportCurrentTable}>
            <Download size={16} /> Export CSV
          </button>
        </AdminActionArea>
      </div>
    </section>
  );
}

function emptyOperations(): Record<OperationState["module"], OperationState> {
  return {
    photos: emptyOperation("photos"),
    taxa: emptyOperation("taxa"),
    mapping: emptyOperation("mapping"),
  };
}

function emptyOperation(module: OperationState["module"]): OperationState {
  return {
    module,
    task_id: null,
    operation: null,
    running: false,
    started_at: null,
    finished_at: null,
    message: "idle",
    processed: 0,
    total: null,
    result: null,
    error: null,
  };
}
