import { useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import {
  Camera,
  ChevronRight,
  Database,
  Download,
  Folder,
  GitBranch,
  Image,
  Map,
  Minus,
  Plus,
  RefreshCw,
  Search,
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  TreePine
} from "lucide-react";
import {
  browsePhotos,
  DirectoryListing,
  downloadTable,
  getMappingRoot,
  getMappingTaxon,
  getPhoto,
  getPhotoRootsMetadata,
  getRoots,
  getMappingMetadata,
  getTaxaMetadata,
  MappingMetadata,
  MappingNode,
  Photo,
  PhotoRootMetadata,
  photoFileUrl,
  runMutation,
  savePhotoRoots,
  saveKnowledgeBasePath,
  searchMappingByBinomial,
  searchMappingByName,
  TaxaMetadata,
  Taxon
} from "./api";

type View = "photos" | "taxonomy" | "map" | "admin";
type ExportModule = "photos" | "taxa" | "mapping";
type TaxonomyState = {
  node: MappingNode;
  trail: Taxon[];
};
type RootRow = PhotoRootMetadata & {
  selected: boolean;
};

const EXPORT_TABLES: Record<ExportModule, string[]> = {
  photos: ["photos", "photos_metadata"],
  taxa: ["taxa", "taxa_metadata"],
  mapping: [
    "photos_taxa_mapping",
    "photos_taxa_mapping_metadata",
    "photos_taxa_mapping_taxa"
  ]
};

const EXPORT_ENDPOINTS: Record<ExportModule, string> = {
  photos: "/photos/export",
  taxa: "/taxa/export",
  mapping: "/mapping/photos-taxa/export"
};

export function App() {
  const [view, setView] = useState<View>("photos");
  const [message, setMessage] = useState("Ready");

  return (
    <div className="shell">
      <header className="app-header">
        <div className="brand">
          <TreePine size={22} />
          <span>PhytoIndex</span>
        </div>
        <nav>
          <NavButton active={view === "photos"} icon={<Folder size={18} />} label="Photos" onClick={() => setView("photos")} />
          <NavButton active={view === "taxonomy"} icon={<GitBranch size={18} />} label="Taxonomy" onClick={() => setView("taxonomy")} />
          <NavButton active={view === "map"} icon={<Map size={18} />} label="Map" onClick={() => setView("map")} />
          <NavButton active={view === "admin"} icon={<Database size={18} />} label="Admin" onClick={() => setView("admin")} />
        </nav>
      </header>

      <main className="workspace">
        {view === "photos" && <PhotosExplorer setMessage={setMessage} />}
        {view === "taxonomy" && <TaxonomyExplorer setMessage={setMessage} />}
        {view === "map" && <MapPage />}
        {view === "admin" && <AdminPanel setMessage={setMessage} />}
      </main>
    </div>
  );
}

function PhotosExplorer({ setMessage }: { setMessage: (message: string) => void }) {
  const [roots, setRoots] = useState<string[]>([]);
  const [root, setRoot] = useState("");
  const [path, setPath] = useState("");
  const [listing, setListing] = useState<DirectoryListing | null>(null);
  const [selected, setSelected] = useState<Photo | null>(null);
  const [backStack, setBackStack] = useState<string[]>([]);
  const [forwardStack, setForwardStack] = useState<string[]>([]);

  useEffect(() => {
    getRoots()
      .then((items) => {
        setRoots(items);
        if (items[0]) {
          setRoot(items[0]);
          setPath("");
        }
      })
      .catch((error) => setMessage(error.message));
  }, [setMessage]);

  useEffect(() => {
    if (!root) {
      return;
    }
    browsePhotos(root, path)
      .then((data) => {
        setListing(data);
        setMessage(`Loaded ${data.directories.length} folders and ${data.files.length} files`);
      })
      .catch((error) => setMessage(error.message));
  }, [root, path, setMessage]);

  const files = listing?.files ?? [];
  const crumbs = useMemo(() => breadcrumb(path), [path]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setSelected(null);
        blurActiveElement();
        return;
      }
      if (!isSelectionKey(event) || isFormElement(event.target)) {
        return;
      }
      event.preventDefault();
      blurActiveElement();
      setSelected((current) => nextPhotoSelection(files, current, event.key === "ArrowDown" ? 1 : -1));
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [files]);

  function openPath(nextPath: string) {
    if (nextPath === path) {
      return;
    }
    setBackStack([...backStack, path]);
    setForwardStack([]);
    setSelected(null);
    setPath(nextPath);
  }

  function goBack() {
    const previous = backStack[backStack.length - 1];
    if (previous === undefined) {
      return;
    }
    setBackStack(backStack.slice(0, -1));
    setForwardStack([path, ...forwardStack]);
    setSelected(null);
    setPath(previous);
  }

  function goForward() {
    const next = forwardStack[0];
    if (next === undefined) {
      return;
    }
    setForwardStack(forwardStack.slice(1));
    setBackStack([...backStack, path]);
    setSelected(null);
    setPath(next);
  }

  return (
    <section className="split">
      <div className="panel" onClick={(event) => shouldClearSelection(event) && setSelected(null)}>
        <div className="toolbar">
          <button title="Back" disabled={!backStack.length} onClick={goBack}><ArrowLeft size={16} /></button>
          <button title="Forward" disabled={!forwardStack.length} onClick={goForward}><ArrowRight size={16} /></button>
          <select value={root} onChange={(event) => { setRoot(event.target.value); setPath(""); setBackStack([]); setForwardStack([]); setSelected(null); }}>
            <option value="">Select root</option>
            {roots.map((item) => <option value={item} key={item}>{item}</option>)}
          </select>
          <button title="Refresh" onClick={() => root && browsePhotos(root, path).then(setListing).catch((error) => setMessage(error.message))}>
            <RefreshCw size={16} />
          </button>
        </div>
        <div className="crumbs">
          <button onClick={() => openPath("")}>root</button>
          {crumbs.map((crumb) => (
            <button key={crumb.path} onClick={() => openPath(crumb.path)}>
              <ChevronRight size={14} /> {crumb.label}
            </button>
          ))}
        </div>
        <div className="browser-list">
          {listing?.directories.map((directory) => (
            <button className="row-button" key={directory} onClick={() => openPath(joinPath(path, directory))}>
              <Folder size={18} /> <span>{directory}</span>
            </button>
          ))}
          {files.map((photo) => (
            <button className={photo.photo_id === selected?.photo_id ? "row-button selected" : "row-button"} key={photo.photo_id} onClick={() => setSelected(togglePhotoSelection(selected, photo))}>
              <Image size={18} /> <span>{photo.filename}</span> <small>{photo.binomial_name ?? "No name"}</small>
            </button>
          ))}
        </div>
      </div>
      {selected ? (
        <PhotoPreview photo={selected} />
      ) : (
        <PhotoGrid photos={files} emptyText="No photos in this folder" onPhotoClick={setSelected} selectedPhotoId={null} onBlankClick={() => setSelected(null)} />
      )}
    </section>
  );
}

function TaxonomyExplorer({ setMessage }: { setMessage: (message: string) => void }) {
  const [state, setState] = useState<TaxonomyState | null>(null);
  const [photos, setPhotos] = useState<Photo[]>([]);
  const [selected, setSelected] = useState<Photo | null>(null);
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<"name" | "binomial">("name");
  const [backStack, setBackStack] = useState<TaxonomyState[]>([]);
  const [forwardStack, setForwardStack] = useState<TaxonomyState[]>([]);

  useEffect(() => {
    getMappingRoot()
      .then((data) => {
        setState({ node: data, trail: [] });
        setMessage(`Loaded ${data.children.length} root taxa`);
      })
      .catch((error) => setMessage(error.message));
  }, [setMessage]);

  useEffect(() => {
    if (!state) {
      return;
    }
    Promise.all(state.node.photo_ids.map(getPhoto))
      .then(setPhotos)
      .catch((error) => setMessage(error.message));
  }, [state, setMessage]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setSelected(null);
        blurActiveElement();
        return;
      }
      if (!isSelectionKey(event) || isFormElement(event.target)) {
        return;
      }
      event.preventDefault();
      blurActiveElement();
      setSelected((current) => nextPhotoSelection(photos, current, event.key === "ArrowDown" ? 1 : -1));
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [photos]);

  function openTaxon(taxon: Taxon, fromTrail?: Taxon[]) {
    getMappingTaxon(taxon.taxon_id)
      .then((node) => {
        const nextState = { node, trail: [...(fromTrail ?? state?.trail ?? []), taxon] };
        pushTaxonomyState(nextState);
      })
      .catch((error) => setMessage(error.message));
  }

  async function search() {
    const trimmed = query.trim();
    if (!trimmed) {
      return;
    }
    try {
      const node = mode === "name"
        ? await searchMappingByName(trimmed)
        : await searchMappingByBinomial(trimmed);
      const trail = await lineageForNode(node);
      pushTaxonomyState({ node, trail });
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  }

  function pushTaxonomyState(nextState: TaxonomyState) {
    if (state) {
      setBackStack([...backStack, state]);
    }
    setForwardStack([]);
    setSelected(null);
    setState(nextState);
  }

  function openTaxonomyRoot() {
    getMappingRoot()
      .then((node) => pushTaxonomyState({ node, trail: [] }))
      .catch((error) => setMessage(error.message));
  }

  function openTrail(index: number) {
    if (index < 0) {
      openTaxonomyRoot();
      return;
    }
    const taxon = state?.trail[index];
    if (!taxon) {
      return;
    }
    openTaxon(taxon, state.trail.slice(0, index));
  }

  function goBack() {
    const previous = backStack[backStack.length - 1];
    if (!previous || !state) {
      return;
    }
    setBackStack(backStack.slice(0, -1));
    setForwardStack([state, ...forwardStack]);
    setSelected(null);
    setState(previous);
  }

  function goForward() {
    const next = forwardStack[0];
    if (!next || !state) {
      return;
    }
    setForwardStack(forwardStack.slice(1));
    setBackStack([...backStack, state]);
    setSelected(null);
    setState(next);
  }

  return (
    <section className="split">
      <div className="panel" onClick={(event) => shouldClearSelection(event) && setSelected(null)}>
        <div className="toolbar">
          <button title="Back" disabled={!backStack.length} onClick={goBack}><ArrowLeft size={16} /></button>
          <button title="Forward" disabled={!forwardStack.length} onClick={goForward}><ArrowRight size={16} /></button>
          <div className="segmented">
            <button className={mode === "name" ? "active" : ""} title="Chinese name" onClick={() => setMode("name")}>中</button>
            <button className={mode === "binomial" ? "active" : ""} title="Binomial name" onClick={() => setMode("binomial")}>Bi</button>
          </div>
          <input value={query} placeholder="Search taxa" onChange={(event) => setQuery(event.target.value)} onKeyDown={(event) => event.key === "Enter" && search()} />
          <button title="Search" onClick={search}><Search size={16} /></button>
        </div>
        <div className="crumbs">
          <button onClick={() => openTrail(-1)}>root</button>
          {state?.trail.map((taxon, index) => (
            <button key={taxon.taxon_id} onClick={() => openTrail(index)}>
              <ChevronRight size={14} /> {taxonCrumbLabel(taxon)}
            </button>
          ))}
        </div>
        <div className="browser-list">
          {state?.node.children.map((taxon) => (
            <button className="row-button" key={taxon.taxon_id} onClick={() => openTaxon(taxon)}>
              <GitBranch size={18} />
              <span className="taxon-line">{taxonLabel(taxon)}</span>
            </button>
          ))}
          {photos.map((photo) => (
            <button className={photo.photo_id === selected?.photo_id ? "row-button selected" : "row-button"} key={photo.photo_id} onClick={() => setSelected(togglePhotoSelection(selected, photo))}>
              <Image size={18} /> <span>{photo.filename}</span> <small>{photo.binomial_name ?? "No name"}</small>
            </button>
          ))}
        </div>
      </div>
      {selected ? (
        <PhotoPreview photo={selected} />
      ) : (
        <PhotoGrid photos={photos} emptyText="No directly mapped photos" onPhotoClick={setSelected} selectedPhotoId={null} onBlankClick={() => setSelected(null)} />
      )}
    </section>
  );
}

function MapPage() {
  return (
    <section className="placeholder-page">
      <Map size={34} />
      <span>Map page is not implemented yet.</span>
    </section>
  );
}

function AdminPanel({ setMessage }: { setMessage: (message: string) => void }) {
  const [rootRows, setRootRows] = useState<RootRow[]>([]);
  const [taxaMetadata, setTaxaMetadata] = useState<TaxaMetadata | null>(null);
  const [knowledgeBasePath, setKnowledgeBasePath] = useState("");
  const [mappingMetadata, setMappingMetadata] = useState<MappingMetadata | null>(null);
  const [exportModule, setExportModule] = useState<ExportModule>("photos");
  const [exportTable, setExportTable] = useState(EXPORT_TABLES.photos[0]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getPhotoRootsMetadata()
      .then((metadata) => setRootRows(metadata.map((row) => ({ ...row, selected: false }))))
      .catch((error) => setMessage(error.message));
    getTaxaMetadata()
      .then((metadata) => {
        setTaxaMetadata(metadata);
        setKnowledgeBasePath(metadata.knowledge_base_path ?? "");
      })
      .catch((error) => setMessage(error.message));
    getMappingMetadata()
      .then(setMappingMetadata)
      .catch((error) => setMessage(error.message));
  }, [setMessage]);

  async function execute(label: string, path: string, body: object) {
    setBusy(true);
    try {
      const result = await runMutation(path, body);
      alert(formatOperationAlert(label, result));
      refreshAdminMetadata();
    } catch (error) {
      alert(`${label} failed: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setBusy(false);
    }
  }

  function refreshAdminMetadata() {
    getPhotoRootsMetadata()
      .then((metadata) => setRootRows((rows) => mergeRootSelection(metadata, rows)))
      .catch((error) => setMessage(error.message));
    getTaxaMetadata()
      .then((metadata) => {
        setTaxaMetadata(metadata);
        setKnowledgeBasePath(metadata.knowledge_base_path ?? "");
      })
      .catch((error) => setMessage(error.message));
    getMappingMetadata()
      .then(setMappingMetadata)
      .catch((error) => setMessage(error.message));
  }

  const exportTables = EXPORT_TABLES[exportModule];
  const selectedRoots = rootRows
    .filter((row) => row.selected && row.root.trim())
    .map((row) => row.root.trim());

  async function persistRoots(nextRows: RootRow[]) {
    setRootRows(nextRows);
    try {
      const metadata = await savePhotoRoots(uniqueRoots(nextRows.map((row) => row.root.trim()).filter(Boolean)));
      setRootRows(mergeRootSelection(metadata, nextRows));
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
        sort_order: rootRows.length,
        selected: true
      }
    ];
    setRootRows(nextRows);
  }

  function removeSelectedRoots() {
    persistRoots(rootRows.filter((row) => !row.selected));
  }

  function moveSelectedRoots(direction: -1 | 1) {
    const nextRows = moveSelectedRows(rootRows, direction);
    persistRoots(nextRows);
  }

  function updateRootValue(index: number, root: string) {
    setRootRows(rootRows.map((row, rowIndex) => rowIndex === index ? { ...row, root } : row));
  }

  function toggleRoot(index: number) {
    setRootRows(rootRows.map((row, rowIndex) => rowIndex === index ? { ...row, selected: !row.selected } : row));
  }

  function syncSelectedRoots(label: string, endpoint: string) {
    if (!selectedRoots.length) {
      setMessage("Select at least one root");
      return;
    }
    execute(label, endpoint, { roots: selectedRoots });
  }

  async function persistKnowledgeBasePath() {
    try {
      const metadata = await saveKnowledgeBasePath(knowledgeBasePath.trim() || null);
      setTaxaMetadata(metadata);
      setKnowledgeBasePath(metadata.knowledge_base_path ?? "");
      setMessage("Knowledge base path saved");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  }

  function selectExportModule(moduleName: ExportModule) {
    const firstTable = EXPORT_TABLES[moduleName][0];
    setExportModule(moduleName);
    setExportTable(firstTable);
  }

  async function exportCurrentTable() {
    setBusy(true);
    setMessage(`Exporting ${exportTable}`);
    try {
      const filename = await downloadTable(EXPORT_ENDPOINTS[exportModule], exportTable);
      setMessage(`Downloaded ${filename}`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="admin-grid">
      <div className="panel photos-admin">
        <h2>Photos</h2>
        <p className="panel-subtitle">Photo roots</p>
        <div className="icon-toolbar">
          <button type="button" title="Add root" onClick={addRoot}><Plus size={16} /></button>
          <button type="button" title="Remove selected roots" onClick={removeSelectedRoots}><Minus size={16} /></button>
          <button type="button" title="Move selected roots up" onClick={() => moveSelectedRoots(-1)}><ArrowUp size={16} /></button>
          <button type="button" title="Move selected roots down" onClick={() => moveSelectedRoots(1)}><ArrowDown size={16} /></button>
        </div>
        <div className="root-table">
          {rootRows.map((row, index) => (
            <div className={row.selected ? "root-row selected" : "root-row"} key={`${row.sort_order}-${index}`}>
              <input
                type="checkbox"
                checked={row.selected}
                onChange={() => toggleRoot(index)}
                aria-label="Select root"
              />
              <input
                value={row.root}
                onChange={(event) => updateRootValue(index, event.target.value)}
                onBlur={() => persistRoots(rootRows)}
                placeholder="/path/to/photos"
              />
              <span>Updated: {row.last_synced_at ?? "not synced"}</span>
            </div>
          ))}
        </div>
        <button type="button" disabled={busy} onClick={() => syncSelectedRoots("Photos update", "/photos/update")}>Update Photos</button>
        <button type="button" disabled={busy} onClick={() => syncSelectedRoots("Photos rebuild", "/photos/rebuild")}>Rebuild Photos</button>
      </div>
      <div className="panel">
        <h2>Taxa</h2>
        <p className="panel-subtitle">Knowledge base path</p>
        <input value={knowledgeBasePath} onChange={(event) => setKnowledgeBasePath(event.target.value)} onBlur={persistKnowledgeBasePath} placeholder="/path/to/plants.xlsm" />
        <div className="metadata-list">
          <div><span>Last synced</span><strong>{taxaMetadata?.last_synced_at ?? "not synced"}</strong></div>
          <div><span>File modified</span><strong>{taxaMetadata?.knowledge_base_modified_at ?? "unknown"}</strong></div>
          <div><span>File size</span><strong>{formatBytes(taxaMetadata?.knowledge_base_size)}</strong></div>
        </div>
        <button type="button" disabled={busy} onClick={() => execute("Taxa update", "/taxa/update", { knowledge_base_path: knowledgeBasePath || null })}>Update Taxa</button>
        <button type="button" disabled={busy} onClick={() => execute("Taxa rebuild", "/taxa/rebuild", { knowledge_base_path: knowledgeBasePath || null })}>Rebuild Taxa</button>
      </div>
      <div className="panel">
        <h2>Mapping</h2>
        <p className="panel-subtitle">Sync metadata</p>
        <div className="metadata-list">
          <div><span>Last synced</span><strong>{mappingMetadata?.last_synced_at ?? "not synced"}</strong></div>
          <div><span>Photos synced</span><strong>{mappingMetadata?.photos_last_synced_at ?? "unknown"}</strong></div>
          <div><span>Taxa synced</span><strong>{mappingMetadata?.taxa_last_synced_at ?? "unknown"}</strong></div>
        </div>
        <button type="button" disabled={busy} onClick={() => execute("Mapping update", "/mapping/photos-taxa/update", {})}>Update Mapping</button>
        <button type="button" disabled={busy} onClick={() => execute("Mapping rebuild", "/mapping/photos-taxa/rebuild", {})}>Rebuild Mapping</button>
      </div>
      <div className="panel">
        <h2>Export Tables</h2>
        <label>Module</label>
        <select value={exportModule} onChange={(event) => selectExportModule(event.target.value as ExportModule)}>
          <option value="photos">Photos</option>
          <option value="taxa">Taxa</option>
          <option value="mapping">Photos-Taxa Mapping</option>
        </select>
        <label>Table</label>
        <select value={exportTable} onChange={(event) => {
          setExportTable(event.target.value);
        }}>
          {exportTables.map((table) => <option value={table} key={table}>{table}</option>)}
        </select>
        <button type="button" disabled={busy} onClick={exportCurrentTable}>
          <Download size={16} /> Export CSV
        </button>
      </div>
    </section>
  );
}

function PhotoPreview({ photo }: { photo: Photo | null }) {
  if (!photo) {
    return <div className="preview empty"><Camera size={34} /><span>Select a photo</span></div>;
  }
  return (
    <div className="preview">
      <div className="preview-image">
        <img src={photoFileUrl(photo.photo_id)} alt={photo.filename} />
      </div>
      <div className="details">
        <dl>
          <dt>Path</dt><dd>{photoDisplayPath(photo)}</dd>
          <dt>GPS</dt><dd>{formatGps(photo)}</dd>
          <dt>Status</dt><dd>{photo.status}</dd>
        </dl>
      </div>
    </div>
  );
}

function PhotoGrid({
  photos,
  emptyText,
  onPhotoClick,
  selectedPhotoId = null,
  onBlankClick
}: {
  photos: Photo[];
  emptyText: string;
  onPhotoClick?: (photo: Photo) => void;
  selectedPhotoId?: number | null;
  onBlankClick?: () => void;
}) {
  if (!photos.length) {
    return <div className="panel empty" onClick={onBlankClick}><Image size={30} /><span>{emptyText}</span></div>;
  }
  return (
    <div className="photo-grid" onClick={(event) => event.currentTarget === event.target && onBlankClick?.()}>
      {photos.map((photo) => (
        <button key={photo.photo_id} className={photo.photo_id === selectedPhotoId ? "photo-tile selected" : "photo-tile"} onClick={() => onPhotoClick?.(photo)}>
          <img src={photoFileUrl(photo.photo_id)} alt={photo.filename} />
          <strong>{photo.binomial_name ?? photo.filename}</strong>
          <span>{photo.location ?? photo.status}</span>
        </button>
      ))}
    </div>
  );
}

function NavButton({ active, icon, label, onClick }: { active: boolean; icon: ReactNode; label: string; onClick: () => void }) {
  return <button className={active ? "active" : ""} onClick={onClick}>{icon}<span>{label}</span></button>;
}

function viewTitle(view: View): string {
  return {
    photos: "Photo Browser",
    taxonomy: "Taxonomy Browser",
    map: "Map",
    admin: "Sync Center"
  }[view];
}

function joinPath(base: string, child: string): string {
  return base ? `${base}/${child}` : child;
}

function photoDisplayPath(photo: Photo): string {
  const separator = photo.root.includes("\\") ? "\\" : "/";
  const root = photo.root.endsWith("/") || photo.root.endsWith("\\")
    ? photo.root.slice(0, -1)
    : photo.root;
  return `${root}${separator}${photo.relative_path.replace(/[\\/]/g, separator)}`;
}

function formatGps(photo: Photo): string {
  if (photo.latitude == null || photo.longitude == null) {
    return "-";
  }
  return `${photo.latitude.toFixed(6)}, ${photo.longitude.toFixed(6)}`;
}

function breadcrumb(path: string) {
  const parts = path.split("/").filter(Boolean);
  return parts.map((part, index) => ({
    label: part,
    path: parts.slice(0, index + 1).join("/")
  }));
}

async function lineageForNode(node: MappingNode): Promise<Taxon[]> {
  if (!node.taxon) {
    return [];
  }
  const lineage: Taxon[] = [node.taxon];
  let parentId = node.taxon.parent_id;
  while (parentId !== null) {
    const parentNode = await getMappingTaxon(parentId);
    if (!parentNode.taxon) {
      break;
    }
    lineage.unshift(parentNode.taxon);
    parentId = parentNode.taxon.parent_id;
  }
  return lineage;
}

function taxonLabel(taxon: Taxon): string {
  return taxon.binomial_name ? `${taxon.name} / ${taxon.binomial_name}` : taxon.name;
}

function taxonCrumbLabel(taxon: Taxon): string {
  return taxon.binomial_name ? `${taxon.name} (${taxon.binomial_name})` : taxon.name;
}

function isSelectionKey(event: KeyboardEvent): boolean {
  return event.key === "ArrowDown" || event.key === "ArrowUp";
}

function isFormElement(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  return ["INPUT", "SELECT", "TEXTAREA"].includes(target.tagName) || target.isContentEditable;
}

function nextPhotoSelection(photos: Photo[], current: Photo | null, direction: 1 | -1): Photo | null {
  if (!photos.length) {
    return null;
  }

  const currentIndex = current
    ? photos.findIndex((photo) => photo.photo_id === current.photo_id)
    : -1;

  if (currentIndex === -1) {
    return direction === 1 ? photos[0] : photos[photos.length - 1];
  }

  const nextIndex = Math.min(
    Math.max(currentIndex + direction, 0),
    photos.length - 1,
  );
  return photos[nextIndex];
}

function togglePhotoSelection(current: Photo | null, next: Photo): Photo | null {
  return current?.photo_id === next.photo_id ? null : next;
}

function shouldClearSelection(event: React.MouseEvent<HTMLElement>): boolean {
  const target = event.target;
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  return !target.closest("button, input, select, textarea, a");
}

function blurActiveElement() {
  if (document.activeElement instanceof HTMLElement) {
    document.activeElement.blur();
  }
}

function uniqueRoots(roots: string[]): string[] {
  return Array.from(new Set(roots));
}

function mergeRootSelection(metadata: PhotoRootMetadata[], previousRows: RootRow[]): RootRow[] {
  const selected = new Set(previousRows.filter((row) => row.selected).map((row) => row.root.trim()));
  return metadata.map((row) => ({
    ...row,
    selected: selected.has(row.root)
  }));
}

function moveSelectedRows(rows: RootRow[], direction: -1 | 1): RootRow[] {
  const nextRows = [...rows];
  const indexes = direction === -1
    ? nextRows.map((row, index) => row.selected ? index : -1).filter((index) => index > 0)
    : nextRows.map((row, index) => row.selected ? index : -1).filter((index) => index >= 0 && index < nextRows.length - 1).reverse();

  for (const index of indexes) {
    const target = index + direction;
    if (nextRows[target].selected) {
      continue;
    }
    const current = nextRows[index];
    nextRows[index] = nextRows[target];
    nextRows[target] = current;
  }

  return nextRows.map((row, index) => ({ ...row, sort_order: index }));
}

function formatOperationAlert(label: string, result: unknown): string {
  if (hasUnmappedPhotos(result)) {
    const photos = result.result.unmapped_photos
      .map((photo) => `${photo.photo_id}: ${photo.root}/${photo.relative_path}`)
      .join("\n");
    return `${label} completed with ${result.result.unmapped} unmapped photos:\n${photos}`;
  }

  const count = operationCount(result);
  if (count !== null) {
    return `${label} succeeded. Changed ${count} rows.`;
  }

  return `${label} succeeded.`;
}

function hasUnmappedPhotos(value: unknown): value is {
  result: { unmapped: number; unmapped_photos: Photo[] };
} {
  if (!value || typeof value !== "object" || !("result" in value)) {
    return false;
  }
  const result = (value as { result?: unknown }).result;
  return Boolean(
    result &&
      typeof result === "object" &&
      "unmapped" in result &&
      Number((result as { unmapped: unknown }).unmapped) > 0 &&
      Array.isArray((result as { unmapped_photos?: unknown }).unmapped_photos),
  );
}

function operationCount(value: unknown): number | null {
  if (!value || typeof value !== "object" || !("result" in value)) {
    return null;
  }
  const result = (value as { result?: unknown }).result;
  if (!result || typeof result !== "object") {
    return null;
  }
  const record = result as Record<string, unknown>;

  if (typeof record.changed === "number") {
    return record.changed;
  }
  if (typeof record.processed === "number") {
    return record.processed;
  }
  if (typeof record.inserted === "number") {
    return record.inserted;
  }
  if (record.results && typeof record.results === "object") {
    return Object.values(record.results as Record<string, Record<string, unknown>>)
      .reduce((total, item) => total + ["new", "updated", "deleted", "inserted"].reduce((sum, key) => (
        sum + (typeof item[key] === "number" ? item[key] : 0)
      ), 0), 0);
  }

  return ["new", "updated", "deleted", "inserted", "mapped"].reduce((total, key) => (
    total + (typeof record[key] === "number" ? record[key] : 0)
  ), 0);
}

function formatBytes(value: number | null | undefined): string {
  if (value == null) {
    return "unknown";
  }
  if (value < 1024) {
    return `${value} B`;
  }
  if (value < 1024 * 1024) {
    return `${(value / 1024).toFixed(1)} KB`;
  }
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}
