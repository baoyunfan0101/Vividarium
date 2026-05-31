import { Camera, Image } from "lucide-react";
import { useEffect, useRef, useState, type MouseEvent, type MutableRefObject, type PointerEvent, type Ref, type SyntheticEvent, type UIEvent, type WheelEvent } from "react";
import type { Photo, Taxon } from "../api";
import { photoFileUrl, photoThumbnailUrl, searchMappingByBinomial } from "../api";
import { photoDisplayPath } from "../lib/photoUtils";
import { lineageForNode } from "../lib/taxonUtils";
import { LoadingOverlay } from "./status";

type PreviewMode = "image" | "details";
type DetailRow = {
  label: string;
  values: string[];
};
const PHOTO_GRID_PADDING = 10;
const PHOTO_GRID_GAP = 8;
const PHOTO_GRID_MIN_COLUMN_WIDTH = 180;

function assignRef<T>(ref: Ref<T> | undefined, value: T | null) {
  if (!ref) {
    return;
  }
  if (typeof ref === "function") {
    ref(value);
    return;
  }
  (ref as MutableRefObject<T | null>).current = value;
}

export function PhotoPreview({
  photo,
  mode = "image",
}: {
  photo: Photo | null;
  mode?: PreviewMode;
}) {
  if (!photo) {
    return <div className="preview empty"><Camera size={34} /><span>Select a photo</span></div>;
  }
  if (mode === "details") {
    return <PhotoDetails photo={photo} />;
  }
  return <PhotoImageViewer photo={photo} />;
}

function PhotoImageViewer({ photo }: { photo: Photo }) {
  const [zoom, setZoom] = useState(1);
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });
  const [imageSize, setImageSize] = useState({
    width: photo.width ?? 0,
    height: photo.height ?? 0,
  });
  const [dragging, setDragging] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ pointerId: number; x: number; y: number; scrollLeft: number; scrollTop: number } | null>(null);
  const pendingScrollRef = useRef<{ xRatio: number; yRatio: number; localX: number; localY: number } | null>(null);

  useEffect(() => {
    setZoom(1);
    setImageSize({ width: photo.width ?? 0, height: photo.height ?? 0 });
    setDragging(false);
    dragRef.current = null;
    pendingScrollRef.current = null;
  }, [photo.photo_id]);

  useEffect(() => {
    const element = containerRef.current;
    if (!element) {
      return;
    }
    function updateSize() {
      if (!element) {
        return;
      }
      setContainerSize({
        width: element.clientWidth,
        height: element.clientHeight,
      });
    }
    updateSize();
    const observer = new ResizeObserver(updateSize);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const element = containerRef.current;
    if (!element) {
      return;
    }
    window.requestAnimationFrame(() => {
      element.scrollLeft = (element.scrollWidth - element.clientWidth) / 2;
      element.scrollTop = (element.scrollHeight - element.clientHeight) / 2;
    });
  }, [photo.photo_id, containerSize, imageSize]);

  useEffect(() => {
    const element = containerRef.current;
    const pending = pendingScrollRef.current;
    if (!element || !pending) {
      return;
    }
    pendingScrollRef.current = null;
    window.requestAnimationFrame(() => {
      element.scrollLeft = pending.xRatio * element.scrollWidth - pending.localX;
      element.scrollTop = pending.yRatio * element.scrollHeight - pending.localY;
    });
  }, [zoom]);

  function handleWheel(event: WheelEvent<HTMLDivElement>) {
    event.preventDefault();
    const direction = event.deltaY < 0 ? 1 : -1;
    zoomAt(event.clientX, event.clientY, (current) => Math.min(Math.max(current + direction * 0.25, 1), 6));
  }

  function handleImageLoad(event: SyntheticEvent<HTMLImageElement>) {
    const image = event.currentTarget;
    setImageSize({ width: image.naturalWidth, height: image.naturalHeight });
  }

  function toggleDefaultZoom(event: MouseEvent<HTMLDivElement>) {
    event.preventDefault();
    zoomAt(event.clientX, event.clientY, (current) => current > 1 ? 1 : 2);
  }

  function zoomAt(clientX: number, clientY: number, nextZoom: (current: number) => number) {
    const element = containerRef.current;
    if (!element) {
      setZoom(nextZoom);
      return;
    }
    const rect = element.getBoundingClientRect();
    const localX = clientX - rect.left;
    const localY = clientY - rect.top;
    const contentX = element.scrollLeft + localX;
    const contentY = element.scrollTop + localY;
    pendingScrollRef.current = {
      xRatio: element.scrollWidth > 0 ? contentX / element.scrollWidth : 0.5,
      yRatio: element.scrollHeight > 0 ? contentY / element.scrollHeight : 0.5,
      localX,
      localY,
    };
    setZoom(nextZoom);
  }

  function handlePointerDown(event: PointerEvent<HTMLDivElement>) {
    const element = containerRef.current;
    if (!element || zoom <= 1) {
      return;
    }
    element.setPointerCapture(event.pointerId);
    dragRef.current = {
      pointerId: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      scrollLeft: element.scrollLeft,
      scrollTop: element.scrollTop,
    };
    setDragging(true);
  }

  function handlePointerMove(event: PointerEvent<HTMLDivElement>) {
    const element = containerRef.current;
    const drag = dragRef.current;
    if (!element || !drag || drag.pointerId !== event.pointerId) {
      return;
    }
    element.scrollLeft = drag.scrollLeft - (event.clientX - drag.x);
    element.scrollTop = drag.scrollTop - (event.clientY - drag.y);
  }

  function endDrag(event: PointerEvent<HTMLDivElement>) {
    if (dragRef.current?.pointerId === event.pointerId) {
      dragRef.current = null;
      setDragging(false);
    }
  }

  const baseScale = imageSize.width > 0 && containerSize.width > 0
    ? containerSize.width / imageSize.width
    : 1;
  const renderedWidth = imageSize.width > 0 ? Math.ceil(imageSize.width * baseScale * zoom) : undefined;
  const renderedHeight = imageSize.height > 0 ? Math.ceil(imageSize.height * baseScale * zoom) : undefined;
  const verticalMargin = renderedHeight && containerSize.height > renderedHeight
    ? Math.floor((containerSize.height - renderedHeight) / 2)
    : 0;

  return (
    <div className="preview">
      <div
        ref={containerRef}
        className={`preview-image ${zoom > 1 ? "zoomed" : ""} ${dragging ? "dragging" : ""}`}
        onWheel={handleWheel}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onDoubleClick={toggleDefaultZoom}
      >
        <img
          src={photoFileUrl(photo)}
          alt={photo.filename}
          draggable={false}
          onLoad={handleImageLoad}
          style={{
            width: renderedWidth,
            height: renderedHeight,
            marginTop: verticalMargin,
            marginBottom: verticalMargin,
          }}
        />
      </div>
    </div>
  );
}

function PhotoDetails({ photo }: { photo: Photo }) {
  const [lineage, setLineage] = useState<Taxon[]>([]);

  useEffect(() => {
    let cancelled = false;
    setLineage([]);
    if (!photo.binomial_name) {
      return () => {
        cancelled = true;
      };
    }

    searchMappingByBinomial(photo.binomial_name)
      .then(lineageForNode)
      .then((items) => {
        if (!cancelled) {
          setLineage(items);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setLineage([]);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [photo.binomial_name]);

  const ordo = lineage.find((taxon) => taxon.rank === "ordo");
  const familia = lineage.find((taxon) => taxon.rank === "familia");
  const genus = lineage.find((taxon) => taxon.rank === "genus");
  const species = lineage.find((taxon) => taxon.rank === "species");
  const rows: DetailRow[] = [
    { label: "ordo", values: formatTaxonValues(ordo) },
    { label: "familia", values: formatTaxonValues(familia) },
    { label: "genus", values: formatTaxonValues(genus) },
    { label: "species", values: formatTaxonValues(species) },
    { label: "path", values: [photoDisplayPath(photo)] },
    { label: "filename", values: [photo.filename] },
    { label: "camera", values: [formatDetailValue(photo.camera)] },
    { label: "captured_at", values: [formatDetailValue(photo.captured_at)] },
    { label: "modified_at", values: [formatDetailValue(photo.modified_at)] },
    { label: "location", values: [formatDetailValue(photo.location)] },
    { label: "longitude / latitude", values: [formatDetailValue(photo.longitude), formatDetailValue(photo.latitude)] },
    { label: "width / height", values: [formatDetailValue(photo.width), formatDetailValue(photo.height)] },
    { label: "exif_json", values: [formatDetailValue(photo.exif_json)] },
    { label: "thumbnail_path", values: [formatDetailValue(photo.thumbnail_path)] },
    { label: "photo_id", values: [formatDetailValue(photo.photo_id)] },
    { label: "status", values: [photo.status] },
  ];

  async function copyRow(value: string) {
    await navigator.clipboard?.writeText(value);
  }

  return (
    <div className="preview photo-details">
      <div className="photo-detail-list">
        {rows.map(({ label, values }) => (
          <div className="photo-detail-row" key={label}>
            <span>{label}</span>
            <div className={values.length > 1 ? "photo-detail-values two-column" : "photo-detail-values"}>
              {values.map((value, index) => (
                <strong title="Double click to copy" key={`${label}:${index}`} onDoubleClick={() => copyRow(value)}>{value}</strong>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function formatTaxonValues(taxon: Taxon | undefined): string[] {
  if (!taxon) {
    return ["", ""];
  }
  return [taxon.name, taxon.binomial_name ?? ""];
}

function formatDetailValue(value: Photo[keyof Photo]): string {
  if (value === null || value === undefined || value === "") {
    return "";
  }
  return String(value);
}

export function PhotoGrid({
  photos,
  emptyText,
  onPhotoClick,
  selectedPhotoId = null,
  onBlankClick,
  loading = false,
  loadingLabel = "Loading",
  subtitleForPhoto,
  scrollRef,
  onScroll
}: {
  photos: Photo[];
  emptyText: string;
  onPhotoClick?: (photo: Photo) => void;
  selectedPhotoId?: number | null;
  onBlankClick?: () => void;
  loading?: boolean;
  loadingLabel?: string;
  subtitleForPhoto?: (photo: Photo) => string | null | undefined;
  scrollRef?: Ref<HTMLDivElement>;
  onScroll?: (event: UIEvent<HTMLDivElement>) => void;
}) {
  const gridRef = useRef<HTMLDivElement | null>(null);
  const [viewport, setViewport] = useState({ width: 0, height: 0 });
  const [scrollTop, setScrollTop] = useState(0);

  useEffect(() => {
    const element = gridRef.current;
    if (!element) {
      return;
    }
    function updateViewport() {
      if (element) {
        setViewport({ width: element.clientWidth, height: element.clientHeight });
        setScrollTop(element.scrollTop);
      }
    }
    updateViewport();
    const observer = new ResizeObserver(updateViewport);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  function setGridRef(element: HTMLDivElement | null) {
    gridRef.current = element;
    assignRef(scrollRef, element);
    if (element) {
      window.requestAnimationFrame(() => {
        setViewport({ width: element.clientWidth, height: element.clientHeight });
        setScrollTop(element.scrollTop);
      });
    }
  }

  function handleScroll(event: UIEvent<HTMLDivElement>) {
    setScrollTop(event.currentTarget.scrollTop);
    onScroll?.(event);
  }

  if (!photos.length) {
    return (
      <div className="panel empty loading-scope" ref={setGridRef} onScroll={handleScroll} onClick={onBlankClick}>
        <Image size={30} />
        <span>{emptyText}</span>
        {loading && <LoadingOverlay label={loadingLabel} />}
      </div>
    );
  }
  const padding = PHOTO_GRID_PADDING;
  const gap = PHOTO_GRID_GAP;
  const minColumnWidth = PHOTO_GRID_MIN_COLUMN_WIDTH;
  const contentWidth = Math.max(0, viewport.width - padding * 2);
  const columns = Math.max(1, Math.floor((contentWidth + gap) / (minColumnWidth + gap)));
  const tileWidth = columns > 1
    ? (contentWidth - gap * (columns - 1)) / columns
    : contentWidth;
  const rowHeight = Math.ceil(Math.max(minColumnWidth, tileWidth) * 0.75 + 58);
  const rowStride = rowHeight + gap;
  const totalRows = Math.ceil(photos.length / columns);
  const totalHeight = padding * 2 + Math.max(0, totalRows * rowStride - gap);
  const startRow = Math.max(0, Math.floor(Math.max(0, scrollTop - padding) / rowStride) - 3);
  const endRow = Math.min(
    totalRows,
    Math.ceil(Math.max(0, scrollTop - padding + viewport.height) / rowStride) + 3,
  );
  const startIndex = startRow * columns;
  const endIndex = Math.min(photos.length, endRow * columns);
  const visiblePhotos = photos.slice(startIndex, endIndex);

  return (
    <div className="photo-grid loading-scope" ref={setGridRef} onScroll={handleScroll} onClick={(event) => event.currentTarget === event.target && onBlankClick?.()}>
      <div className="photo-grid-spacer" style={{ height: totalHeight }}>
        <div
          className="photo-grid-window"
          style={{
            gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
            left: padding,
            right: padding,
            transform: `translateY(${padding + startRow * rowStride}px)`,
          }}
        >
          {visiblePhotos.map((photo) => (
            <button key={photo.photo_id} disabled={loading} className={photo.photo_id === selectedPhotoId ? "photo-tile selected" : "photo-tile"} style={{ height: rowHeight }} onClick={() => onPhotoClick?.(photo)}>
              <LazyThumbnail photo={photo} />
              <strong>{photo.binomial_name ?? photo.filename}</strong>
              <span>{subtitleForPhoto?.(photo) ?? ""}</span>
            </button>
          ))}
        </div>
      </div>
      {loading && <LoadingOverlay label={loadingLabel} />}
    </div>
  );
}

export function scrollPhotoGridToIndex(element: HTMLDivElement | null, index: number, itemCount: number): number | null {
  if (!element || index < 0 || itemCount <= 0) {
    return null;
  }
  const contentWidth = Math.max(0, element.clientWidth - PHOTO_GRID_PADDING * 2);
  const columns = Math.max(1, Math.floor((contentWidth + PHOTO_GRID_GAP) / (PHOTO_GRID_MIN_COLUMN_WIDTH + PHOTO_GRID_GAP)));
  const tileWidth = columns > 1
    ? (contentWidth - PHOTO_GRID_GAP * (columns - 1)) / columns
    : contentWidth;
  const rowHeight = Math.ceil(Math.max(PHOTO_GRID_MIN_COLUMN_WIDTH, tileWidth) * 0.75 + 58);
  const rowStride = rowHeight + PHOTO_GRID_GAP;
  const row = Math.floor(index / columns);
  const maxScrollTop = Math.max(0, element.scrollHeight - element.clientHeight);
  const nextScrollTop = Math.min(
    maxScrollTop,
    Math.max(0, PHOTO_GRID_PADDING + row * rowStride - Math.max(0, element.clientHeight - rowHeight) / 2),
  );
  element.scrollTop = nextScrollTop;
  return nextScrollTop;
}

function LazyThumbnail({ photo }: { photo: Photo }) {
  const [visible, setVisible] = useState(false);
  const imageRef = useRef<HTMLImageElement>(null);

  useEffect(() => {
    const image = imageRef.current;
    if (!image || visible) {
      return;
    }

    function isNearViewport(element: HTMLElement): boolean {
      const margin = 600;
      const rect = element.getBoundingClientRect();
      return (
        rect.bottom >= -margin &&
        rect.top <= window.innerHeight + margin &&
        rect.right >= -margin &&
        rect.left <= window.innerWidth + margin
      );
    }

    if (isNearViewport(image)) {
      setVisible(true);
      return;
    }

    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          setVisible(true);
          observer.disconnect();
        }
      },
      { rootMargin: "600px" },
    );
    observer.observe(image);
    window.requestAnimationFrame(() => {
      if (isNearViewport(image)) {
        setVisible(true);
        observer.disconnect();
      }
    });
    return () => observer.disconnect();
  }, [visible]);

  return (
    <img
      ref={imageRef}
      src={visible ? photoThumbnailUrl(photo) : undefined}
      alt={photo.filename}
      loading="lazy"
      decoding="async"
    />
  );
}
