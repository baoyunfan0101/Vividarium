import { useEffect, useMemo, useRef, useState, type UIEvent } from "react";
import { ChevronRight, Folder, Image } from "lucide-react";
import { browsePhotosPage, getRoots, searchMappingByBinomial, type DirectoryListingPage, type Photo } from "../../bridge";
import { PhotoGrid, PhotoPreview, scrollPhotoGridToIndex } from "../../components/photo";
import { LoadingOverlay } from "../../components/status";
import { VirtualList } from "../../components/virtual";
import { blurActiveElement, findTypeSelectIndex, isFormElement, isSelectionKey, isTypeSelectKey, nextTypeSelect, scrollListItemIntoView, shouldClearSelection, type TypeSelectState } from "../../lib/browserUtils";
import { breadcrumb, joinPath } from "../../lib/pathUtils";
import { readStorage, writeStorage } from "../../lib/storage";
import { useResizableSplit } from "../../lib/useResizableSplit";

const PHOTOS_STATE_KEY = "phytoindex.photos.explorer";
const LIST_ITEM_HEIGHT = 37;
const BROWSE_PAGE_LIMIT = 180;
const LOAD_MORE_DISTANCE = 900;

type PhotosBrowserItem =
  | { type: "directory"; directory: string }
  | { type: "photo"; photo: Photo };

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
  const [listing, setListing] = useState<DirectoryListingPage | null>(null);
  const [selected, setSelected] = useState<Photo | null>(null);
  const [selectedPhotoKey, setSelectedPhotoKey] = useState<string | null>(cachedState.selectedPhotoKey ?? null);
  const [selectedView, setSelectedView] = useState<"image" | "details">(cachedState.selectedView ?? "image");
  const [listScrollTop, setListScrollTop] = useState(cachedState.listScrollTop);
  const [gridScrollTop, setGridScrollTop] = useState(cachedState.gridScrollTop);
  const [nameByBinomial, setNameByBinomial] = useState<Record<string, string>>({});
  const [listingLoading, setListingLoading] = useState(false);
  const [pageLoading, setPageLoading] = useState(false);
  const [activeBrowserKey, setActiveBrowserKey] = useState<string | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  const typeSelectRef = useRef<TypeSelectState>({ query: "", updatedAt: 0 });
  const pageLoadingRef = useRef(false);
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
        setActiveBrowserKey(null);
        blurActiveElement();
        return;
      }
      if (isFormElement(event.target)) {
        return;
      }
      if (isTypeSelectKey(event)) {
        event.preventDefault();
        blurActiveElement();
        typeSelectBrowserItem(event.key);
        return;
      }
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        blurActiveElement();
        openActiveBrowserItem();
        return;
      }
      if (!isSelectionKey(event)) {
        return;
      }
      event.preventDefault();
      blurActiveElement();
      selectNextBrowserItem(event.key === "ArrowDown" ? 1 : -1);
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [browserItems, files, selected, activeBrowserKey]);

  function openPath(nextPath: string) {
    if (nextPath === path) {
      return;
    }
    selectPhoto(null);
    setActiveBrowserKey(null);
    setListScrollTop(0);
    setGridScrollTop(0);
    setPath(nextPath);
  }

  function selectPhoto(photo: Photo | null, reveal: { list?: boolean; grid?: boolean } = {}) {
    setSelected(photo);
    setSelectedPhotoKey(photo ? photoKey(photo) : null);
    setActiveBrowserKey(photo ? photosBrowserItemKey({ type: "photo", photo }) : null);
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

  function selectBrowserItem(item: PhotosBrowserItem, reveal: { list?: boolean; grid?: boolean } = {}) {
    setActiveBrowserKey(photosBrowserItemKey(item));
    if (item.type === "directory") {
      setSelected(null);
      setSelectedPhotoKey(null);
      setSelectedView("image");
      return;
    }
    selectPhoto(item.photo, reveal);
  }

  function selectNextBrowserItem(direction: 1 | -1) {
    if (!browserItems.length) {
      return;
    }
    const currentIndex = activeBrowserKey
      ? browserItems.findIndex((item) => photosBrowserItemKey(item) === activeBrowserKey)
      : -1;
    const nextIndex = currentIndex < 0
      ? direction === 1 ? 0 : browserItems.length - 1
      : Math.min(Math.max(currentIndex + direction, 0), browserItems.length - 1);
    const item = browserItems[nextIndex];
    selectBrowserItem(item, { grid: true });
    const nextScrollTop = scrollListItemIntoView(listRef.current, nextIndex, LIST_ITEM_HEIGHT);
    if (nextScrollTop !== null) {
      setListScrollTop(nextScrollTop);
    }
  }

  function openActiveBrowserItem() {
    const item = activeBrowserKey
      ? browserItems.find((candidate) => photosBrowserItemKey(candidate) === activeBrowserKey)
      : null;
    if (!item) {
      return;
    }
    if (item.type === "directory") {
      openPath(joinPath(path, item.directory));
      return;
    }
    activatePhoto(item.photo, "list");
  }

  function typeSelectBrowserItem(key: string) {
    const typeSelect = nextTypeSelect(typeSelectRef.current, key);
    typeSelectRef.current = typeSelect.state;
    const currentIndex = activeBrowserKey
      ? browserItems.findIndex((item) => photosBrowserItemKey(item) === activeBrowserKey)
      : -1;
    const startIndex = typeSelect.shouldCycle && currentIndex >= 0 ? currentIndex + 1 : 0;
    const matchIndex = findTypeSelectIndex(
      browserItems,
      typeSelect.query,
      photosBrowserItemLabels,
      startIndex,
    );
    if (matchIndex < 0) {
      return;
    }
    const item = browserItems[matchIndex];
    selectBrowserItem(item);
    const nextScrollTop = scrollListItemIntoView(listRef.current, matchIndex, LIST_ITEM_HEIGHT);
    if (nextScrollTop !== null) {
      setListScrollTop(nextScrollTop);
    }
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

  function handleListScroll(event: UIEvent<HTMLDivElement>) {
    setListScrollTop(event.currentTarget.scrollTop);
    maybeLoadNextPage(event.currentTarget);
  }

  function handleGridScroll(event: UIEvent<HTMLDivElement>) {
    setGridScrollTop(event.currentTarget.scrollTop);
    maybeLoadNextPage(event.currentTarget);
  }

  function maybeLoadNextPage(element: HTMLDivElement) {
    if (!listing?.next_cursor || listingLoading || pageLoadingRef.current) {
      return;
    }
    const distanceToBottom = element.scrollHeight - element.scrollTop - element.clientHeight;
    if (distanceToBottom <= LOAD_MORE_DISTANCE) {
      loadNextListingPage();
    }
  }

  async function loadListing(nextRoot: string, nextPath: string) {
    setListingLoading(true);
    try {
      const data = await browsePhotosPage(nextRoot, nextPath, null, BROWSE_PAGE_LIMIT);
      setListing(data);
      setMessage(`Loaded ${data.directory_count} folders and ${data.file_count} files`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setListingLoading(false);
    }
  }

  async function loadNextListingPage() {
    if (!root || !listing?.next_cursor || pageLoadingRef.current) {
      return;
    }
    const requestCursor = listing.next_cursor;
    pageLoadingRef.current = true;
    setPageLoading(true);
    try {
      const data = await browsePhotosPage(root, path, requestCursor, BROWSE_PAGE_LIMIT);
      setListing((current) => {
        if (!current || current.root !== data.root || current.relative_dir !== data.relative_dir || current.next_cursor !== requestCursor) {
          return current;
        }
        return mergeDirectoryPages(current, data);
      });
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      pageLoadingRef.current = false;
      setPageLoading(false);
    }
  }

  const selectedId = selected?.photo_id ?? null;

  return (
    <section
      className="split resizable-split"
      ref={splitRef}
      style={{ gridTemplateColumns: `minmax(220px, ${splitRatio}%) 5px minmax(0, 1fr)` }}
    >
      <div className="panel browser-panel loading-scope" onClick={(event) => shouldClearSelection(event) && selectPhoto(null)}>
        <div className="toolbar">
          <select
            value={root}
            disabled={listingLoading}
            onChange={(event) => {
              event.currentTarget.blur();
              setRoot(event.target.value);
              setPath("");
              selectPhoto(null);
              setListScrollTop(0);
              setGridScrollTop(0);
            }}
          >
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
          itemHeight={LIST_ITEM_HEIGHT}
          scrollRef={listRef}
          onScroll={handleListScroll}
          itemKey={(index) => {
            const item = browserItems[index];
            return item.type === "directory" ? `dir:${item.directory}` : `photo:${item.photo.photo_id}`;
          }}
          renderItem={(index) => {
            const item = browserItems[index];
            if (item.type === "directory") {
              return (
                <button
                  className={activeBrowserKey === photosBrowserItemKey(item) ? "row-button type-selected" : "row-button"}
                  disabled={listingLoading}
                  onClick={() => openPath(joinPath(path, item.directory))}
                >
                  <Folder size={18} /> <span>{item.directory}</span>
                </button>
              );
            }
            const itemKey = photosBrowserItemKey(item);
            return (
              <button
                className={
                  item.photo.photo_id === selected?.photo_id
                    ? "row-button file-row selected"
                    : activeBrowserKey === itemKey
                      ? "row-button file-row type-selected"
                      : "row-button file-row"
                }
                disabled={listingLoading}
                onClick={() => activatePhoto(item.photo, "list")}
              >
                <Image size={18} /> <span>{item.photo.filename}</span>
              </button>
            );
          }}
        />
        <div className="browser-status">
          {(listing?.directory_count ?? 0)} folders · {listing?.file_count ?? 0} photos
        </div>
        {listingLoading && <LoadingOverlay label="Loading photos" />}
      </div>
      <div className="split-resizer" role="separator" aria-label="Resize photos panels" onPointerDown={beginResize} />
      {selected ? (
        <PhotoPreview photo={selected} mode={selectedView} />
      ) : (
        <PhotoGrid
          photos={files}
          emptyText="No photos in this folder"
          loading={listingLoading || pageLoading}
          loadingLabel={listingLoading ? "Loading photos" : "Loading more"}
          onPhotoClick={(photo) => activatePhoto(photo, "grid")}
          selectedPhotoId={selectedId}
          onBlankClick={() => selectPhoto(null)}
          subtitleForPhoto={(photo) => photo.binomial_name ? nameByBinomial[photo.binomial_name] : ""}
          scrollRef={gridRef}
          onScroll={handleGridScroll}
        />
      )}
    </section>
  );
}

function photoKey(photo: Photo): string {
  return `${photo.root}\n${photo.relative_path}`;
}

function photosBrowserItemKey(item: PhotosBrowserItem): string {
  return item.type === "directory" ? `dir:${item.directory}` : `photo:${item.photo.photo_id}`;
}

function photosBrowserItemLabels(item: PhotosBrowserItem): string[] {
  return item.type === "directory" ? [item.directory] : [item.photo.filename];
}

function mergeDirectoryPages(
  current: DirectoryListingPage,
  next: DirectoryListingPage,
): DirectoryListingPage {
  return {
    ...next,
    directories: [...current.directories, ...next.directories],
    files: [...current.files, ...next.files],
  };
}
