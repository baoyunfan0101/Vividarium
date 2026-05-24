import { Camera, Image } from "lucide-react";
import { useEffect, useRef, useState, type MouseEvent, type PointerEvent, type Ref, type SyntheticEvent, type UIEvent, type WheelEvent } from "react";
import type { Photo } from "../api";
import { photoFileUrl, photoThumbnailUrl } from "../api";
import { photoDisplayPath } from "../lib/photoUtils";
import { LoadingOverlay } from "./status";

type PreviewMode = "image" | "details";

const PHOTO_DETAIL_FIELDS: Array<keyof Photo> = [
  "photo_id",
  "root",
  "relative_path",
  "parent_dir",
  "path_depth",
  "filename",
  "binomial_name",
  "captured_at",
  "location",
  "camera",
  "width",
  "height",
  "file_size",
  "modified_at",
  "longitude",
  "latitude",
  "exif_json",
  "thumbnail_path",
  "status",
];

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

  const baseScale = imageSize.width > 0 && imageSize.height > 0 && containerSize.width > 0 && containerSize.height > 0
    ? Math.max(containerSize.width / imageSize.width, containerSize.height / imageSize.height)
    : 1;
  const renderedWidth = imageSize.width > 0 ? Math.ceil(imageSize.width * baseScale * zoom) : undefined;
  const renderedHeight = imageSize.height > 0 ? Math.ceil(imageSize.height * baseScale * zoom) : undefined;

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
          style={{ width: renderedWidth, height: renderedHeight }}
        />
      </div>
    </div>
  );
}

function PhotoDetails({ photo }: { photo: Photo }) {
  const rows = [
    ["path", photoDisplayPath(photo)],
    ...PHOTO_DETAIL_FIELDS.map((field) => [field, formatDetailValue(photo[field])]),
  ] as Array<[string, string]>;

  async function copyRow(value: string) {
    await navigator.clipboard?.writeText(value);
  }

  return (
    <div className="preview photo-details">
      <div className="photo-detail-list">
        {rows.map(([label, value]) => (
          <div className="photo-detail-row" key={label}>
            <span>{label}</span>
            <strong title="Double click to copy" onDoubleClick={() => copyRow(value)}>{value}</strong>
          </div>
        ))}
      </div>
    </div>
  );
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
  if (!photos.length) {
    return (
      <div className="panel empty loading-scope" ref={scrollRef} onScroll={onScroll} onClick={onBlankClick}>
        <Image size={30} />
        <span>{emptyText}</span>
        {loading && <LoadingOverlay label={loadingLabel} />}
      </div>
    );
  }
  return (
    <div className="photo-grid loading-scope" ref={scrollRef} onScroll={onScroll} onClick={(event) => event.currentTarget === event.target && onBlankClick?.()}>
      {photos.map((photo) => (
        <button key={photo.photo_id} disabled={loading} className={photo.photo_id === selectedPhotoId ? "photo-tile selected" : "photo-tile"} onClick={() => onPhotoClick?.(photo)}>
          <LazyThumbnail photo={photo} />
          <strong>{photo.binomial_name ?? photo.filename}</strong>
          <span>{subtitleForPhoto?.(photo) ?? ""}</span>
        </button>
      ))}
      {loading && <LoadingOverlay label={loadingLabel} />}
    </div>
  );
}

function LazyThumbnail({ photo }: { photo: Photo }) {
  const [visible, setVisible] = useState(false);
  const imageRef = useRef<HTMLImageElement>(null);

  useEffect(() => {
    const image = imageRef.current;
    if (!image || visible) {
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
