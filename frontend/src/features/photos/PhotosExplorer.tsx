import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronRight, Folder, Image } from "lucide-react";
import { browsePhotos, getRoots, searchMappingByBinomial, type DirectoryListing, type Photo } from "../../api";
import { PhotoGrid, PhotoPreview, scrollPhotoGridToIndex } from "../../components/photo";
import { LoadingOverlay } from "../../components/status";
import { VirtualList } from "../../components/virtual";
import { blurActiveElement, isFormElement, isSelectionKey, nextPhotoSelection, scrollListItemIntoView, shouldClearSelection } from "../../lib/browserUtils";
import { breadcrumb, joinPath } from "../../lib/pathUtils";
import { readStorage, writeStorage } from "../../lib/storage";
import { useResizableSplit } from "../../lib/useResizableSplit";

const PHOTOS_STATE_KEY = "phytoindex.photos.explorer";
const LIST_ITEM_HEIGHT = 37;

type CachedPhotosState = {
  root: string;
  path: string;
  selectedPhotoKey: string | null;
  selectedView: "image" | "details";
  listScrollTop: number;
  gridScrollTop: number;
  splitRatio: number;
};

export function PhotosExplorer({ setMessage }: { setMessage: (message: string) => void }) {
  const cachedState = readStorage<CachedPhotosState>(PHOTOS_STATE_KEY, {
    root: "",
    path: "",
    selectedPhotoKey: null,
    selectedView: "image",
    listScrollTop: 0,
    gridScrollTop: 0,
    splitRatio: 34
  });
  const [roots, setRoots] = useState<string[]>([]);
  const [root, setRoot] = useState(cachedState.root);
  const [path, setPath] = useState(cachedState.path);
  const [listing, setListing] = useState<DirectoryListing | null>(null);
  const [selected, setSelected] = useState<Photo | null>(null);
  const [selectedPhotoKey, setSelectedPhotoKey] = useState<string | null>(cachedState.selectedPhotoKey ?? null);
  const [selectedView, setSelectedView] = useState<"image" | "details">(cachedState.selectedView ?? "image");
  const [listScrollTop, setListScrollTop] = useState(cachedState.listScrollTop);
  const [gridScrollTop, setGridScrollTop] = useState(cachedState.gridScrollTop);
  const [nameByBinomial, setNameByBinomial] = useState<Record<string, string>>({});
  const [listingLoading, setListingLoading] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  const { beginResize, splitRatio, splitRef } = useResizableSplit(cachedState.splitRatio ?? 34);

  useEffect(() => {
    getRoots()
      .then((items) => {
        setRoots(items);
        if (items.includes(cachedState.root)) {
          return;
        }
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
    loadListing(root, path);
  }, [root, path, setMessage]);

  const files = listing?.files ?? [];
  const crumbs = useMemo(() => breadcrumb(path), [path]);
  const browserItems = useMemo(() => [
    ...(listing?.directories ?? []).map((directory) => ({ type: "directory" as const, directory })),
    ...files.map((photo) => ({ type: "photo" as const, photo })),
  ], [listing, files]);

  useEffect(() => {
    writeStorage(PHOTOS_STATE_KEY, {
      root,
      path,
      selectedPhotoKey,
      selectedView,
      listScrollTop,
      gridScrollTop,
      splitRatio
    });
  }, [root, path, selectedPhotoKey, selectedView, listScrollTop, gridScrollTop, splitRatio]);

  useEffect(() => {
    if (!selectedPhotoKey) {
      setSelected(null);
      return;
    }
    const nextSelected = files.find((photo) => photoKey(photo) === selectedPhotoKey) ?? null;
    setSelected(nextSelected);
    if (nextSelected) {
      revealPhoto(nextSelected, { list: true, grid: true });
    }
  }, [files, selectedPhotoKey]);

  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listScrollTop;
    }
  }, [listing]);

  useEffect(() => {
    if (gridRef.current) {
      gridRef.current.scrollTop = gridScrollTop;
    }
  }, [listing, selected]);

  useEffect(() => {
    const missingNames = Array.from(new Set(
      files
        .map((photo) => photo.binomial_name)
        .filter((name): name is string => Boolean(name && !(name in nameByBinomial)))
    ));
    if (!missingNames.length) {
      return;
    }

    Promise.all(
      missingNames.map(async (binomialName) => {
        try {
          const node = await searchMappingByBinomial(binomialName);
          return [binomialName, node.taxon?.name ?? ""] as const;
        } catch {
          return [binomialName, ""] as const;
        }
      })
    ).then((items) => {
      setNameByBinomial((current) => ({
        ...current,
        ...Object.fromEntries(items)
      }));
    });
  }, [files, nameByBinomial]);

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
      const next = nextPhotoSelection(files, selected, event.key === "ArrowDown" ? 1 : -1);
      selectPhoto(next, { list: true, grid: true });
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [files, selected]);

  function openPath(nextPath: string) {
    if (nextPath === path) {
      return;
    }
    selectPhoto(null);
    setListScrollTop(0);
    setGridScrollTop(0);
    setPath(nextPath);
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
    const fileIndex = files.findIndex((item) => item.photo_id === photo.photo_id);
    if (fileIndex < 0) {
      return;
    }
    if (reveal.list) {
      const listIndex = (listing?.directories.length ?? 0) + fileIndex;
      const nextScrollTop = scrollListItemIntoView(listRef.current, listIndex, LIST_ITEM_HEIGHT);
      if (nextScrollTop !== null) {
        setListScrollTop(nextScrollTop);
      }
    }
    if (reveal.grid) {
      const nextScrollTop = scrollPhotoGridToIndex(gridRef.current, fileIndex, files.length);
      if (nextScrollTop !== null) {
        setGridScrollTop(nextScrollTop);
      }
    }
  }

  async function loadListing(nextRoot: string, nextPath: string) {
    setListingLoading(true);
    try {
      const data = await browsePhotos(nextRoot, nextPath);
      setListing(data);
      setMessage(`Loaded ${data.directories.length} folders and ${data.files.length} files`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setListingLoading(false);
    }
  }

  const selectedId = selected?.photo_id ?? null;

  return (
    <section className="split resizable-split" ref={splitRef} style={{ gridTemplateColumns: `minmax(220px, ${splitRatio}%) 5px minmax(0, 1fr)` }}>
      <div className="panel browser-panel loading-scope" onClick={(event) => shouldClearSelection(event) && selectPhoto(null)}>
        <div className="toolbar">
          <select value={root} disabled={listingLoading} onChange={(event) => { setRoot(event.target.value); setPath(""); selectPhoto(null); setListScrollTop(0); setGridScrollTop(0); }}>
            <option value="">Select root</option>
            {roots.map((item) => <option value={item} key={item}>{item}</option>)}
          </select>
        </div>
        <div className="crumbs">
          <button disabled={listingLoading} onClick={() => openPath("")}>root</button>
          {crumbs.map((crumb) => (
            <button key={crumb.path} disabled={listingLoading} onClick={() => openPath(crumb.path)}>
              <ChevronRight size={14} /> {crumb.label}
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
            return item.type === "directory" ? `dir:${item.directory}` : `photo:${item.photo.photo_id}`;
          }}
          renderItem={(index) => {
            const item = browserItems[index];
            if (item.type === "directory") {
              return (
                <button className="row-button" disabled={listingLoading} onClick={() => openPath(joinPath(path, item.directory))}>
                  <Folder size={18} /> <span>{item.directory}</span>
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
          {(listing?.directories.length ?? 0)} folders · {files.length} photos
        </div>
        {listingLoading && <LoadingOverlay label="Loading photos" />}
      </div>
      <div className="split-resizer" role="separator" aria-label="Resize photos panels" onPointerDown={beginResize} />
      {selected ? (
        <PhotoPreview photo={selected} mode={selectedView} />
      ) : (
        <PhotoGrid photos={files} emptyText="No photos in this folder" loading={listingLoading} loadingLabel="Loading photos" onPhotoClick={(photo) => activatePhoto(photo, "grid")} selectedPhotoId={selectedId} onBlankClick={() => selectPhoto(null)} subtitleForPhoto={(photo) => photo.binomial_name ? nameByBinomial[photo.binomial_name] : ""} scrollRef={gridRef} onScroll={(event) => setGridScrollTop(event.currentTarget.scrollTop)} />
      )}
    </section>
  );
}

function photoKey(photo: Photo): string {
  return `${photo.root}\n${photo.relative_path}`;
}
