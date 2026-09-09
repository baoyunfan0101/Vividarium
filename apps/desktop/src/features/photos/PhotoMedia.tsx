import { Camera, LoaderCircle } from "lucide-react";
import {
  useEffect,
  useRef,
  useState,
  type MouseEvent,
  type PointerEvent,
  type SyntheticEvent,
  type WheelEvent,
} from "react";
import { photoUrl, type Photo } from "../../api/photos";
import { usePhotoLibraryIdentity } from "./PhotoLibraryIdentity";

type Size = { width: number; height: number };
type Pan = { x: number; y: number };
type LoadedImage = { photoId: number; size: Size };

export function PhotoStage({
  photo,
  compact = false,
  onContextMenu,
}: {
  photo: Photo | null;
  compact?: boolean;
  onContextMenu?: (event: MouseEvent, photo: Photo) => void;
}) {
  const libraryUuid = usePhotoLibraryIdentity();
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ pointerId: number; x: number; y: number; panX: number; panY: number } | null>(null);
  const viewRef = useRef({
    zoom: 1,
    pan: { x: 0, y: 0 },
    baseDisplaySize: { width: 0, height: 0 },
    containerSize: { width: 0, height: 0 },
  });
  const [loadedImage, setLoadedImage] = useState<LoadedImage | null>(null);
  const [containerSize, setContainerSize] = useState<Size>({ width: 0, height: 0 });
  const [zoom, setZoom] = useState(1);
  const [pan, setPan] = useState<Pan>({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    setZoom(1);
    setPan({ x: 0, y: 0 });
    setDragging(false);
    dragRef.current = null;
    viewRef.current = {
      zoom: 1,
      pan: { x: 0, y: 0 },
      baseDisplaySize: { width: 0, height: 0 },
      containerSize: { width: 0, height: 0 },
    };
  }, [photo?.photo_id]);

  useEffect(() => {
    const element = containerRef.current;
    if (!element) return;
    const update = () => {
      setContainerSize({ width: element.clientWidth, height: element.clientHeight });
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [photo?.photo_id]);

  if (!photo) {
    return (
      <div className="photo-stage empty">
        <Camera size={28} />
        <span>Select a photo</span>
      </div>
    );
  }

  const photoId = photo.photo_id;
  const imageSize = loadedImage?.photoId === photo.photo_id
    ? loadedImage.size
    : { width: 0, height: 0 };
  const loaded = loadedImage?.photoId === photo.photo_id;
  const baseScale = fitScale(imageSize, containerSize);
  const baseDisplaySize = {
    width: Math.max(0, Math.round(imageSize.width * baseScale)),
    height: Math.max(0, Math.round(imageSize.height * baseScale)),
  };
  const clampedPan = clampPan(pan, zoom, baseDisplaySize, containerSize);
  const canPan = canPanImage(zoom, baseDisplaySize, containerSize);
  const ready = loaded && baseDisplaySize.width > 0 && baseDisplaySize.height > 0;
  viewRef.current = { zoom, pan: clampedPan, baseDisplaySize, containerSize };

  function handleImageLoad(event: SyntheticEvent<HTMLImageElement>) {
    const image = event.currentTarget;
    setLoadedImage({
      photoId,
      size: { width: image.naturalWidth, height: image.naturalHeight },
    });
  }

  function handleWheel(event: WheelEvent<HTMLDivElement>) {
    event.preventDefault();
    const direction = event.deltaY < 0 ? 1 : -1;
    zoomAt(event.clientX, event.clientY, (current) => current + direction * 0.25);
  }

  function toggleDefaultZoom(event: MouseEvent<HTMLDivElement>) {
    event.preventDefault();
    zoomAt(event.clientX, event.clientY, (current) => (current > 1 ? 1 : 2));
  }

  function zoomAt(clientX: number, clientY: number, nextZoom: (current: number) => number) {
    const element = containerRef.current;
    const view = viewRef.current;
    const targetZoom = clampZoom(nextZoom(view.zoom));
    if (!element || view.baseDisplaySize.width <= 0 || view.baseDisplaySize.height <= 0) {
      viewRef.current = { ...view, zoom: targetZoom };
      setZoom(targetZoom);
      return;
    }
    const rect = element.getBoundingClientRect();
    const localX = clientX - rect.left - rect.width / 2;
    const localY = clientY - rect.top - rect.height / 2;
    const targetPan = clampPan({
      x: localX - ((localX - view.pan.x) / view.zoom) * targetZoom,
      y: localY - ((localY - view.pan.y) / view.zoom) * targetZoom,
    }, targetZoom, view.baseDisplaySize, view.containerSize);
    viewRef.current = { ...view, zoom: targetZoom, pan: targetPan };
    setZoom(targetZoom);
    setPan(targetPan);
  }

  function handlePointerDown(event: PointerEvent<HTMLDivElement>) {
    if (!canPan || event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = {
      pointerId: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      panX: clampedPan.x,
      panY: clampedPan.y,
    };
    setDragging(true);
  }

  function handlePointerMove(event: PointerEvent<HTMLDivElement>) {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const view = viewRef.current;
    const targetPan = clampPan({
      x: drag.panX + event.clientX - drag.x,
      y: drag.panY + event.clientY - drag.y,
    }, view.zoom, view.baseDisplaySize, view.containerSize);
    viewRef.current = { ...view, pan: targetPan };
    setPan(targetPan);
  }

  function endDrag(event: PointerEvent<HTMLDivElement>) {
    if (dragRef.current?.pointerId !== event.pointerId) return;
    dragRef.current = null;
    setDragging(false);
  }

  return (
    <div
      className={`photo-stage${compact ? " compact" : ""}`}
      onContextMenu={(event) => {
        event.preventDefault();
        onContextMenu?.(event, photo);
      }}
    >
      {!loaded && <LoaderCircle className="spin photo-loader" size={20} />}
      <div
        ref={containerRef}
        className={`photo-stage-frame${canPan ? " pannable" : ""}${dragging ? " dragging" : ""}`}
        onDoubleClick={toggleDefaultZoom}
        onPointerCancel={endDrag}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={endDrag}
        onWheel={handleWheel}
      >
        <img
          key={photo.photo_id}
          className="photo-stage-image"
          src={photoUrl(photo, false, libraryUuid)}
          alt={photo.filename}
          draggable={false}
          onLoad={handleImageLoad}
          style={{
            width: baseDisplaySize.width,
            height: baseDisplaySize.height,
            opacity: ready ? 1 : 0,
            transform: `translate(-50%, -50%) translate(${clampedPan.x}px, ${clampedPan.y}px) scale(${zoom})`,
          }}
        />
      </div>
    </div>
  );
}

function fitScale(imageSize: Size, containerSize: Size): number {
  if (
    imageSize.width <= 0
    || imageSize.height <= 0
    || containerSize.width <= 0
    || containerSize.height <= 0
  ) {
    return 1;
  }
  return Math.min(containerSize.width / imageSize.width, containerSize.height / imageSize.height);
}

function clampZoom(value: number): number {
  return Math.min(Math.max(value, 1), 6);
}

function clampPan(pan: Pan, zoom: number, baseDisplaySize: Size, containerSize: Size): Pan {
  const maxX = Math.max(0, (baseDisplaySize.width * zoom - containerSize.width) / 2);
  const maxY = Math.max(0, (baseDisplaySize.height * zoom - containerSize.height) / 2);
  return {
    x: Math.min(Math.max(pan.x, -maxX), maxX),
    y: Math.min(Math.max(pan.y, -maxY), maxY),
  };
}

function canPanImage(zoom: number, baseDisplaySize: Size, containerSize: Size): boolean {
  return (
    baseDisplaySize.width * zoom > containerSize.width + 1
    || baseDisplaySize.height * zoom > containerSize.height + 1
  );
}

export function PhotoThumb({
  photo,
  selected,
  onClick,
  onDoubleClick,
  onContextMenu,
}: {
  photo: Photo;
  selected: boolean;
  onClick: () => void;
  onDoubleClick?: () => void;
  onContextMenu?: (event: MouseEvent) => void;
}) {
  const libraryUuid = usePhotoLibraryIdentity();
  return (
    <button
      className={`photo-thumb${selected ? " selected" : ""}`}
      type="button"
      aria-label={photo.filename}
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      onContextMenu={(event) => {
        event.preventDefault();
        onContextMenu?.(event);
      }}
    >
      <img src={photoUrl(photo, true, libraryUuid)} alt="" loading="lazy" draggable={false} />
    </button>
  );
}
