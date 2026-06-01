import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronRight, GitBranch, Image, Search } from "lucide-react";
import { getMappingRoot, getMappingTaxon, getPhoto, searchMappingByBinomial, searchMappingByName, suggestMappingTaxa, type MappingNode, type Photo, type Taxon, type TaxonSuggestion } from "../../api";
import { PhotoGrid, PhotoPreview, scrollPhotoGridToIndex } from "../../components/photo";
import { LoadingOverlay } from "../../components/status";
import { VirtualList } from "../../components/virtual";
import { blurActiveElement, isFormElement, isSelectionKey, nextPhotoSelection, scrollListItemIntoView, shouldClearSelection } from "../../lib/browserUtils";
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
  selectedPhotoKey: string | null;
  selectedView: "image" | "details";
  listScrollTop: number;
  gridScrollTop: number;
  splitRatio: number;
};

const TAXONOMY_STATE_KEY = "phytoindex.taxonomy.explorer";
const LIST_ITEM_HEIGHT = 37;

export function TaxonomyExplorer({ setMessage }: { setMessage: (message: string) => void }) {
  const cachedState = readStorage<CachedTaxonomyState>(TAXONOMY_STATE_KEY, {
    state: null,
    query: "",
    selectedPhotoKey: null,
    selectedView: "image",
    listScrollTop: 0,
    gridScrollTop: 0,
    splitRatio: 34
  });
  const [state, setState] = useState<TaxonomyState | null>(cachedState.state);
  const [photos, setPhotos] = useState<Photo[]>([]);
  const [selected, setSelected] = useState<Photo | null>(null);
  const [selectedPhotoKey, setSelectedPhotoKey] = useState<string | null>(cachedState.selectedPhotoKey ?? null);
  const [selectedView, setSelectedView] = useState<"image" | "details">(cachedState.selectedView ?? "image");
  const [query, setQuery] = useState(cachedState.query);
  const [suggestions, setSuggestions] = useState<TaxonSuggestion[]>([]);
  const [suggestionsOpen, setSuggestionsOpen] = useState(false);
  const [activeSuggestionIndex, setActiveSuggestionIndex] = useState(-1);
  const [searchFocused, setSearchFocused] = useState(false);
  const [listScrollTop, setListScrollTop] = useState(cachedState.listScrollTop);
  const [gridScrollTop, setGridScrollTop] = useState(cachedState.gridScrollTop);
  const [listingLoading, setListingLoading] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  const suppressSuggestionRef = useRef(false);
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
      selectedPhotoKey,
      selectedView,
      listScrollTop,
      gridScrollTop,
      splitRatio
    });
  }, [state, query, selectedPhotoKey, selectedView, listScrollTop, gridScrollTop, splitRatio]);

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
    if (nextSelected) {
      revealPhoto(nextSelected, { list: true, grid: true });
    }
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
    if (!trimmed || !searchFocused) {
      setSuggestions([]);
      setSuggestionsOpen(false);
      return;
    }
    if (suppressSuggestionRef.current) {
      suppressSuggestionRef.current = false;
      setSuggestions([]);
      setSuggestionsOpen(false);
      setActiveSuggestionIndex(-1);
      return;
    }
    const timer = window.setTimeout(() => {
      suggestTaxaAcrossNames(trimmed)
        .then((items) => {
          setSuggestions(items);
          setSuggestionsOpen(items.length > 0);
          setActiveSuggestionIndex(items.length > 0 ? 0 : -1);
        })
        .catch(() => {
          setSuggestions([]);
          setSuggestionsOpen(false);
          setActiveSuggestionIndex(-1);
        });
    }, 180);
    return () => window.clearTimeout(timer);
  }, [query, searchFocused]);

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
      const next = nextPhotoSelection(photos, selected, event.key === "ArrowDown" ? 1 : -1);
      selectPhoto(next, { list: true, grid: true });
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [photos, selected]);

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
    setActiveSuggestionIndex(-1);
    setListingLoading(true);
    try {
      const node = await searchTaxaAcrossNames(trimmed);
      const trail = await lineageForNode(node);
      pushTaxonomyState({ node, trail });
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setListingLoading(false);
    }
  }

  async function applySuggestion(suggestion: TaxonSuggestion) {
    const value = suggestion.name || suggestion.binomial_name || "";
    suppressSuggestionRef.current = true;
    setQuery(value);
    setSuggestionsOpen(false);
    setActiveSuggestionIndex(-1);
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

  function selectPhoto(photo: Photo | null, reveal: { list?: boolean; grid?: boolean } = {}) {
    setSelected(photo);
    setSelectedPhotoKey(photo ? photoKey(photo) : null);
    setSelectedView("image");
    if (photo) {
      revealPhoto(photo, reveal);
    }
  }

  function activatePhoto(photo: Photo, source: "list" | "grid") {
    if (selected?.photo_id === photo.photo_id) {
      setSelectedView((view) => view === "image" ? "details" : "image");
      return;
    }
    selectPhoto(photo, {
      list: source === "grid",
      grid: source === "list",
    });
  }

  function revealPhoto(photo: Photo, reveal: { list?: boolean; grid?: boolean }) {
    const photoIndex = photos.findIndex((item) => item.photo_id === photo.photo_id);
    if (photoIndex < 0) {
      return;
    }
    if (reveal.list) {
      const listIndex = (state?.node.children.length ?? 0) + photoIndex;
      const nextScrollTop = scrollListItemIntoView(listRef.current, listIndex, LIST_ITEM_HEIGHT);
      if (nextScrollTop !== null) {
        setListScrollTop(nextScrollTop);
      }
    }
    if (reveal.grid) {
      const nextScrollTop = scrollPhotoGridToIndex(gridRef.current, photoIndex, photos.length);
      if (nextScrollTop !== null) {
        setGridScrollTop(nextScrollTop);
      }
    }
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
  const browserItems = useMemo(() => [
    ...(state?.node.children ?? []).map((taxon) => ({ type: "taxon" as const, taxon })),
    ...photos.map((photo) => ({ type: "photo" as const, photo })),
  ], [state, photos]);

  return (
    <section className="split resizable-split" ref={splitRef} style={{ gridTemplateColumns: `minmax(220px, ${splitRatio}%) 5px minmax(0, 1fr)` }}>
      <div className="panel browser-panel loading-scope" onClick={(event) => shouldClearSelection(event) && selectPhoto(null)}>
        <div className="toolbar">
          <div className="autocomplete">
            <input
              value={query}
              disabled={listingLoading}
              placeholder="Search Chinese or binomial name"
              onChange={(event) => setQuery(event.target.value)}
              onFocus={() => {
                setSearchFocused(true);
                if (suggestions.length > 0) {
                  setSuggestionsOpen(true);
                }
              }}
              onBlur={() => window.setTimeout(() => {
                setSearchFocused(false);
                setSuggestionsOpen(false);
              }, 120)}
              onKeyDown={(event) => {
                if (suggestionsOpen && suggestions.length > 0 && event.key === "ArrowDown") {
                  event.preventDefault();
                  setActiveSuggestionIndex((index) => Math.min(index + 1, suggestions.length - 1));
                  return;
                }
                if (suggestionsOpen && suggestions.length > 0 && event.key === "ArrowUp") {
                  event.preventDefault();
                  setActiveSuggestionIndex((index) => Math.max(index - 1, 0));
                  return;
                }
                if (suggestionsOpen && suggestions.length > 0 && event.key === "Enter") {
                  event.preventDefault();
                  applySuggestion(suggestions[Math.max(activeSuggestionIndex, 0)]);
                  return;
                }
                if (event.key === "Enter") {
                  search();
                }
              }}
            />
            {suggestionsOpen && (
              <div className="suggestion-list">
                {suggestions.map((suggestion, index) => (
                  <button
                    type="button"
                    className={index === activeSuggestionIndex ? "active" : ""}
                    key={suggestion.taxon_id}
                    disabled={listingLoading}
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => applySuggestion(suggestion)}
                  >
                    <span>{suggestion.name}</span>
                    <small>{suggestion.binomial_name}</small>
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
        <VirtualList
          className="browser-list"
          itemCount={browserItems.length}
          itemHeight={37}
          scrollRef={listRef}
          onScroll={(event) => setListScrollTop(event.currentTarget.scrollTop)}
          itemKey={(index) => {
            const item = browserItems[index];
            return item.type === "taxon" ? `taxon:${item.taxon.taxon_id}` : `photo:${item.photo.photo_id}`;
          }}
          renderItem={(index) => {
            const item = browserItems[index];
            if (item.type === "taxon") {
              return (
                <button className="row-button" disabled={listingLoading} onClick={() => openTaxon(item.taxon)}>
                  <GitBranch size={18} />
                  <span className="taxon-line">{taxonLabel(item.taxon)}</span>
                </button>
              );
            }
            return (
              <button className={item.photo.photo_id === selected?.photo_id ? "row-button file-row selected" : "row-button file-row"} disabled={listingLoading} onClick={() => activatePhoto(item.photo, "list")}>
                <Image size={18} /> <span>{item.photo.filename}</span>
              </button>
            );
          }}
        />
        <div className="browser-status">
          {(state?.node.children.length ?? 0)} taxa · {photos.length} photos
        </div>
        {listingLoading && <LoadingOverlay label="Loading taxonomy" />}
      </div>
      <div className="split-resizer" role="separator" aria-label="Resize taxonomy panels" onPointerDown={beginResize} />
      {selected ? (
        <PhotoPreview photo={selected} mode={selectedView} />
      ) : (
        <PhotoGrid photos={photos} emptyText="No directly mapped photos" loading={listingLoading} loadingLabel="Loading taxonomy" onPhotoClick={(photo) => activatePhoto(photo, "grid")} selectedPhotoId={selectedId} onBlankClick={() => selectPhoto(null)} subtitleForPhoto={() => state?.node.taxon?.name ?? ""} scrollRef={gridRef} onScroll={(event) => setGridScrollTop(event.currentTarget.scrollTop)} />
      )}
    </section>
  );
}

async function suggestTaxaAcrossNames(query: string): Promise<TaxonSuggestion[]> {
  const [byName, byBinomial] = await Promise.all([
    suggestMappingTaxa(query, "name"),
    suggestMappingTaxa(query, "binomial"),
  ]);
  const seen = new Set<number>();
  return [...byName, ...byBinomial].filter((taxon) => {
    if (seen.has(taxon.taxon_id)) {
      return false;
    }
    seen.add(taxon.taxon_id);
    return true;
  });
}

async function searchTaxaAcrossNames(query: string): Promise<MappingNode> {
  const byName = await searchMappingByName(query);
  if (mappingNodeHasContent(byName)) {
    return byName;
  }
  return searchMappingByBinomial(query);
}

function mappingNodeHasContent(node: MappingNode): boolean {
  return Boolean(node.taxon || node.photo_ids.length || node.children.length);
}

function photoKey(photo: Photo): string {
  return `${photo.root}\n${photo.relative_path}`;
}
