import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronRight, Folder, Image } from "lucide-react";
import { browsePhotos, getRoots, searchMappingByBinomial, type DirectoryListing, type Photo } from "../../api";
import { PhotoGrid, PhotoPreview } from "../../components/photo";
import { LoadingOverlay } from "../../components/status";
import { blurActiveElement, isFormElement, isSelectionKey, nextPhotoSelection, shouldClearSelection } from "../../lib/browserUtils";
import { breadcrumb, joinPath } from "../../lib/pathUtils";
import { readStorage, writeStorage } from "../../lib/storage";
import { useResizableSplit } from "../../lib/useResizableSplit";

const PHOTOS_STATE_KEY = "phytoindex.photos.explorer";

type CachedPhotosState = {
  root: string;
  path: string;
  selectedPhotoKey: string | null;
  listScrollTop: number;
  gridScrollTop: number;
  splitRatio: number;
};

export function PhotosExplorer({ setMessage }: { setMessage: (message: string) => void }) {
  const cachedState = readStorage<CachedPhotosState>(PHOTOS_STATE_KEY, {
    root: "",
    path: "",
    selectedPhotoKey: null,
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
  const [selectedView, setSelectedView] = useState<"image" | "details">("image");
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

  useEffect(() => {
    writeStorage(PHOTOS_STATE_KEY, {
      root,
      path,
      selectedPhotoKey,
      listScrollTop,
      gridScrollTop,
      splitRatio
    });
  }, [root, path, selectedPhotoKey, listScrollTop, gridScrollTop, splitRatio]);

  useEffect(() => {
    if (!selectedPhotoKey) {
      setSelected(null);
      return;
    }
    const nextSelected = files.find((photo) => photoKey(photo) === selectedPhotoKey) ?? null;
    setSelected(nextSelected);
  }, [files, selectedPhotoKey]);

  useEffect(() => {
    if (listRef.current) {
      listRef.current.scrollTop = listScrollTop;
    }
    if (gridRef.current) {
      gridRef.current.scrollTop = gridScrollTop;
    }
  }, [listing, selected, listScrollTop, gridScrollTop]);

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
      setSelected((current) => {
        const next = nextPhotoSelection(files, current, event.key === "ArrowDown" ? 1 : -1);
        setSelectedPhotoKey(next ? photoKey(next) : null);
        setSelectedView("image");
        return next;
      });
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [files]);

  function openPath(nextPath: string) {
    if (nextPath === path) {
      return;
    }
    selectPhoto(null);
    setPath(nextPath);
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
    <section className="split resizable-split" ref={splitRef} style={{ gridTemplateColumns: `minmax(220px, ${splitRatio}%) 6px minmax(0, 1fr)` }}>
      <div className="panel browser-panel loading-scope" onClick={(event) => shouldClearSelection(event) && setSelected(null)}>
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
        <div className="browser-list" ref={listRef} onScroll={(event) => setListScrollTop(event.currentTarget.scrollTop)}>
          {listing?.directories.map((directory) => (
            <button className="row-button" disabled={listingLoading} key={directory} onClick={() => openPath(joinPath(path, directory))}>
              <Folder size={18} /> <span>{directory}</span>
            </button>
          ))}
          {files.map((photo) => (
            <button className={photo.photo_id === selected?.photo_id ? "row-button file-row selected" : "row-button file-row"} disabled={listingLoading} key={photo.photo_id} onClick={() => activatePhoto(photo)}>
              <Image size={18} /> <span>{photo.filename}</span>
            </button>
          ))}
        </div>
        <div className="browser-status">
          {(listing?.directories.length ?? 0)} folders · {files.length} photos
        </div>
        {listingLoading && <LoadingOverlay label="Loading photos" />}
      </div>
      <div className="split-resizer" role="separator" aria-label="Resize photos panels" onPointerDown={beginResize} />
      {selected ? (
        <PhotoPreview photo={selected} mode={selectedView} />
      ) : (
        <PhotoGrid photos={files} emptyText="No photos in this folder" loading={listingLoading} loadingLabel="Loading photos" onPhotoClick={activatePhoto} selectedPhotoId={selectedId} onBlankClick={() => selectPhoto(null)} subtitleForPhoto={(photo) => photo.binomial_name ? nameByBinomial[photo.binomial_name] : ""} scrollRef={gridRef} onScroll={(event) => setGridScrollTop(event.currentTarget.scrollTop)} />
      )}
    </section>
  );
}

function photoKey(photo: Photo): string {
  return `${photo.root}\n${photo.relative_path}`;
}
