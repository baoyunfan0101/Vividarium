import { useEffect, useRef, useState } from "react";
import { ChevronRight, GitBranch, Image, Search } from "lucide-react";
import { getMappingRoot, getMappingTaxon, getPhoto, searchMappingByBinomial, searchMappingByName, suggestMappingTaxa, type MappingNode, type Photo, type Taxon, type TaxonSuggestion } from "../../api";
import { PhotoGrid, PhotoPreview } from "../../components/photo";
import { LoadingOverlay } from "../../components/status";
import { blurActiveElement, isFormElement, isSelectionKey, nextPhotoSelection, shouldClearSelection } from "../../lib/browserUtils";
import { readStorage, writeStorage } from "../../lib/storage";
import { lineageForNode, taxonCrumbLabel, taxonLabel } from "../../lib/taxonUtils";
import { useResizableSplit } from "../../lib/useResizableSplit";

type TaxonomyState = {
  node: MappingNode;
  trail: Taxon[];
};

type CachedTaxonomyState = {
  state: TaxonomyState | null;
  query: string;
  mode: "name" | "binomial";
  selectedPhotoKey: string | null;
  listScrollTop: number;
  gridScrollTop: number;
  splitRatio: number;
};

const TAXONOMY_STATE_KEY = "phytoindex.taxonomy.explorer";

export function TaxonomyExplorer({ setMessage }: { setMessage: (message: string) => void }) {
  const cachedState = readStorage<CachedTaxonomyState>(TAXONOMY_STATE_KEY, {
    state: null,
    query: "",
    mode: "name",
    selectedPhotoKey: null,
    listScrollTop: 0,
    gridScrollTop: 0,
    splitRatio: 34
  });
  const [state, setState] = useState<TaxonomyState | null>(cachedState.state);
  const [photos, setPhotos] = useState<Photo[]>([]);
  const [selected, setSelected] = useState<Photo | null>(null);
  const [selectedPhotoKey, setSelectedPhotoKey] = useState<string | null>(cachedState.selectedPhotoKey ?? null);
  const [selectedView, setSelectedView] = useState<"image" | "details">("image");
  const [query, setQuery] = useState(cachedState.query);
  const [mode, setMode] = useState<"name" | "binomial">(cachedState.mode);
  const [suggestions, setSuggestions] = useState<TaxonSuggestion[]>([]);
  const [suggestionsOpen, setSuggestionsOpen] = useState(false);
  const [listScrollTop, setListScrollTop] = useState(cachedState.listScrollTop);
  const [gridScrollTop, setGridScrollTop] = useState(cachedState.gridScrollTop);
  const [listingLoading, setListingLoading] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  const { beginResize, splitRatio, splitRef } = useResizableSplit(cachedState.splitRatio ?? 34);

  useEffect(() => {
    if (state) {
      return;
    }
    setListingLoading(true);
    getMappingRoot()
      .then((data) => {
        setState({ node: data, trail: [] });
        setMessage(`Loaded ${data.children.length} root taxa`);
      })
      .catch((error) => setMessage(error.message))
      .finally(() => setListingLoading(false));
  }, [setMessage]);

  useEffect(() => {
    writeStorage(TAXONOMY_STATE_KEY, {
      state,
      query,
      mode,
      selectedPhotoKey,
      listScrollTop,
      gridScrollTop,
      splitRatio
    });
  }, [state, query, mode, selectedPhotoKey, listScrollTop, gridScrollTop, splitRatio]);

  useEffect(() => {
    if (!state) {
      return;
    }
    setListingLoading(true);
    Promise.all(state.node.photo_ids.map(getPhoto))
      .then(setPhotos)
      .catch((error) => setMessage(error.message))
      .finally(() => setListingLoading(false));
  }, [state, setMessage]);

  useEffect(() => {
    if (!selectedPhotoKey) {
      setSelected(null);
      return;
    }
    const nextSelected = photos.find((photo) => photoKey(photo) === selectedPhotoKey) ?? null;
    setSelected(nextSelected);
  }, [photos, selectedPhotoKey]);

  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listScrollTop;
    }
  }, [state]);

  useEffect(() => {
    if (gridRef.current) {
      gridRef.current.scrollTop = gridScrollTop;
    }
  }, [state, photos, selected]);

  useEffect(() => {
    const trimmed = query.trim();
    if (!trimmed) {
      setSuggestions([]);
      setSuggestionsOpen(false);
      return;
    }
    const timer = window.setTimeout(() => {
      suggestMappingTaxa(trimmed, mode)
        .then((items) => {
          setSuggestions(items);
          setSuggestionsOpen(items.length > 0);
        })
        .catch(() => {
          setSuggestions([]);
          setSuggestionsOpen(false);
        });
    }, 180);
    return () => window.clearTimeout(timer);
  }, [query, mode]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        selectPhoto(null);
        blurActiveElement();
        return;
      }
      if (!isSelectionKey(event) || isFormElement(event.target)) {
        return;
      }
      event.preventDefault();
      blurActiveElement();
      setSelected((current) => {
        const next = nextPhotoSelection(photos, current, event.key === "ArrowDown" ? 1 : -1);
        setSelectedPhotoKey(next ? photoKey(next) : null);
        setSelectedView("image");
        return next;
      });
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [photos]);

  function openTaxon(taxon: Taxon, fromTrail?: Taxon[]) {
    setListingLoading(true);
    getMappingTaxon(taxon.taxon_id)
      .then((node) => {
        const nextState = { node, trail: [...(fromTrail ?? state?.trail ?? []), taxon] };
        pushTaxonomyState(nextState);
      })
      .catch((error) => setMessage(error.message))
      .finally(() => setListingLoading(false));
  }

  async function search() {
    const trimmed = query.trim();
    if (!trimmed) {
      return;
    }
    setSuggestionsOpen(false);
    setListingLoading(true);
    try {
      const node = mode === "name"
        ? await searchMappingByName(trimmed)
        : await searchMappingByBinomial(trimmed);
      const trail = await lineageForNode(node);
      pushTaxonomyState({ node, trail });
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setListingLoading(false);
    }
  }

  async function applySuggestion(suggestion: TaxonSuggestion) {
    const value = mode === "name" ? suggestion.name : suggestion.binomial_name ?? suggestion.name;
    setQuery(value);
    setSuggestionsOpen(false);
    setListingLoading(true);
    try {
      const node = await getMappingTaxon(suggestion.taxon_id);
      const trail = await lineageForNode(node);
      pushTaxonomyState({ node, trail });
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setListingLoading(false);
    }
  }

  function pushTaxonomyState(nextState: TaxonomyState) {
    selectPhoto(null);
    setListScrollTop(0);
    setGridScrollTop(0);
    setState(nextState);
  }

  function selectPhoto(photo: Photo | null) {
    setSelected(photo);
    setSelectedPhotoKey(photo ? photoKey(photo) : null);
    setSelectedView("image");
  }

  function activatePhoto(photo: Photo) {
    if (selected?.photo_id === photo.photo_id) {
      setSelectedView((view) => view === "image" ? "details" : "image");
      return;
    }
    selectPhoto(photo);
  }

  function openTaxonomyRoot() {
    setListingLoading(true);
    getMappingRoot()
      .then((node) => pushTaxonomyState({ node, trail: [] }))
      .catch((error) => setMessage(error.message))
      .finally(() => setListingLoading(false));
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

  const selectedId = selected?.photo_id ?? null;

  return (
    <section className="split resizable-split" ref={splitRef} style={{ gridTemplateColumns: `minmax(220px, ${splitRatio}%) 6px minmax(0, 1fr)` }}>
      <div className="panel browser-panel loading-scope" onClick={(event) => shouldClearSelection(event) && setSelected(null)}>
        <div className="toolbar">
          <div className="segmented">
            <button className={mode === "name" ? "active" : ""} disabled={listingLoading} title="Chinese name" onClick={() => setMode("name")}>中</button>
            <button className={mode === "binomial" ? "active" : ""} disabled={listingLoading} title="Binomial name" onClick={() => setMode("binomial")}>Bi</button>
          </div>
          <div className="autocomplete">
            <input
              value={query}
              disabled={listingLoading}
              placeholder="Search taxa"
              onChange={(event) => setQuery(event.target.value)}
              onFocus={() => suggestions.length > 0 && setSuggestionsOpen(true)}
              onBlur={() => window.setTimeout(() => setSuggestionsOpen(false), 120)}
              onKeyDown={(event) => event.key === "Enter" && search()}
            />
            {suggestionsOpen && (
              <div className="suggestion-list">
                {suggestions.map((suggestion) => (
                  <button
                    type="button"
                    key={suggestion.taxon_id}
                    disabled={listingLoading}
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => applySuggestion(suggestion)}
                  >
                    <span>{mode === "name" ? suggestion.name : suggestion.binomial_name}</span>
                    <small>{mode === "name" ? suggestion.binomial_name : suggestion.name}</small>
                  </button>
                ))}
              </div>
            )}
          </div>
          <button title="Search" disabled={listingLoading} onClick={search}><Search size={16} /></button>
        </div>
        <div className="crumbs">
          <button disabled={listingLoading} onClick={() => openTrail(-1)}>root</button>
          {state?.trail.map((taxon, index) => (
            <button key={taxon.taxon_id} disabled={listingLoading} onClick={() => openTrail(index)}>
              <ChevronRight size={14} /> {taxonCrumbLabel(taxon)}
            </button>
          ))}
        </div>
        <div className="browser-list" ref={listRef} onScroll={(event) => setListScrollTop(event.currentTarget.scrollTop)}>
          {state?.node.children.map((taxon) => (
            <button className="row-button" disabled={listingLoading} key={taxon.taxon_id} onClick={() => openTaxon(taxon)}>
              <GitBranch size={18} />
              <span className="taxon-line">{taxonLabel(taxon)}</span>
            </button>
          ))}
          {photos.map((photo) => (
            <button className={photo.photo_id === selected?.photo_id ? "row-button file-row selected" : "row-button file-row"} disabled={listingLoading} key={photo.photo_id} onClick={() => activatePhoto(photo)}>
              <Image size={18} /> <span>{photo.filename}</span>
            </button>
          ))}
        </div>
        <div className="browser-status">
          {(state?.node.children.length ?? 0)} taxa · {photos.length} photos
        </div>
        {listingLoading && <LoadingOverlay label="Loading taxonomy" />}
      </div>
      <div className="split-resizer" role="separator" aria-label="Resize taxonomy panels" onPointerDown={beginResize} />
      {selected ? (
        <PhotoPreview photo={selected} mode={selectedView} />
      ) : (
        <PhotoGrid photos={photos} emptyText="No directly mapped photos" loading={listingLoading} loadingLabel="Loading taxonomy" onPhotoClick={activatePhoto} selectedPhotoId={selectedId} onBlankClick={() => selectPhoto(null)} subtitleForPhoto={() => state?.node.taxon?.name ?? ""} scrollRef={gridRef} onScroll={(event) => setGridScrollTop(event.currentTarget.scrollTop)} />
      )}
    </section>
  );
}

function photoKey(photo: Photo): string {
  return `${photo.root}\n${photo.relative_path}`;
}
