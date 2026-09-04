import { Image as ImageIcon, LayoutGrid } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type MouseEvent } from "react";
import type { Photo } from "../../api/photos";
import { IconButton, VirtualGrid } from "../../shared/ui";
import { PhotoStage, PhotoThumb } from "./PhotoMedia";
import { isPhotoFullscreenActive } from "./photoFullscreenState";
import { createPhotoActivationController } from "./photoActivation";

export type PhotoDisplayMode = "thumbnails" | "image";

export function usePhotoActivation({
  onSelect,
  onOpenImage,
  onOpenFullscreen,
}: {
  onSelect: (photo: Photo) => void;
  onOpenImage: (photo: Photo) => void;
  onOpenFullscreen: (photo: Photo) => void;
}) {
  const callbacksRef = useRef({ onSelect, onOpenImage, onOpenFullscreen });
  callbacksRef.current = { onSelect, onOpenImage, onOpenFullscreen };
  const activationRef = useRef<ReturnType<typeof createPhotoActivationController> | null>(null);
  if (activationRef.current === null) {
    activationRef.current = createPhotoActivationController({
      onSelect: (photo) => callbacksRef.current.onSelect(photo),
      onOpenImage: (photo) => callbacksRef.current.onOpenImage(photo),
      onOpenFullscreen: (photo) => callbacksRef.current.onOpenFullscreen(photo),
    });
  }

  useEffect(() => () => activationRef.current?.dispose(), []);

  return activationRef.current;
}

export function usePhotoDisplayMode({
  onEscapeToThumbnails,
  onEnterFullscreen,
}: {
  onEscapeToThumbnails?: () => void;
  onEnterFullscreen?: () => void;
} = {}) {
  const [mode, setMode] = useState<PhotoDisplayMode>("thumbnails");
  const modeRef = useRef(mode);
  const onEscapeToThumbnailsRef = useRef(onEscapeToThumbnails);
  const onEnterFullscreenRef = useRef(onEnterFullscreen);
  modeRef.current = mode;
  onEscapeToThumbnailsRef.current = onEscapeToThumbnails;
  onEnterFullscreenRef.current = onEnterFullscreen;

  useEffect(() => {
    const returnToThumbnails = (event: KeyboardEvent) => {
      if (modeRef.current === "image" && event.key === "Enter" && !event.defaultPrevented) {
        const target = event.target;
        if (target instanceof HTMLElement && target.closest("button, input, textarea, select, [contenteditable=true]")) return;
        event.preventDefault();
        onEnterFullscreenRef.current?.();
        return;
      }
      if (modeRef.current !== "image" || event.key !== "Escape" || event.defaultPrevented) return;
      if (isPhotoFullscreenActive()) return;
      event.preventDefault();
      setMode("thumbnails");
      onEscapeToThumbnailsRef.current?.();
    };
    window.addEventListener("keydown", returnToThumbnails, true);
    return () => window.removeEventListener("keydown", returnToThumbnails, true);
  }, []);

  return [mode, setMode] as const;
}

export function PhotoDisplayToggle({
  mode,
  onChange,
}: {
  mode: PhotoDisplayMode;
  onChange: (mode: PhotoDisplayMode) => void;
}) {
  return (
    <div className="photo-display-toggle" role="group" aria-label="Photo display">
      <IconButton
        aria-label="Thumbnails"
        className={mode === "thumbnails" ? "active" : ""}
        size="small"
        title="Thumbnails"
        onClick={() => onChange("thumbnails")}
      >
        <LayoutGrid size={14} />
      </IconButton>
      <IconButton
        aria-label="Image"
        className={mode === "image" ? "active" : ""}
        size="small"
        title="Image"
        onClick={() => onChange("image")}
      >
        <ImageIcon size={14} />
      </IconButton>
    </div>
  );
}

export function PhotoDisplay({
  photos,
  selected,
  mode,
  stateKey,
  onModeChange,
  onSelect,
  onClickPhoto,
  onDoubleClickPhoto,
  onNearEnd,
  onContextMenu,
}: {
  photos: Photo[];
  selected: Photo | null;
  mode: PhotoDisplayMode;
  stateKey: string;
  onModeChange: (mode: PhotoDisplayMode) => void;
  onSelect: (photo: Photo) => void;
  onClickPhoto: (photo: Photo) => void;
  onDoubleClickPhoto: (photo: Photo) => void;
  onNearEnd?: () => void;
  onContextMenu?: (event: MouseEvent, photo: Photo) => void;
}) {
  const activeIndex = selected
    ? photos.findIndex((photo) => photo.photo_id === selected.photo_id)
    : -1;

  if (mode === "image") {
    return <PhotoStage photo={selected} onContextMenu={onContextMenu} />;
  }

  return (
    <VirtualGrid
      stateKey={stateKey}
      items={photos}
      activeIndex={activeIndex}
      itemKey={(photo) => photo.photo_id}
      onActivateActive={() => {
        if (activeIndex >= 0) onModeChange("image");
      }}
      onMoveActive={(index) => onSelect(photos[index])}
      onNearEnd={onNearEnd}
      renderItem={(photo) => (
        <PhotoThumb
          photo={photo}
          selected={selected?.photo_id === photo.photo_id}
          onClick={() => onClickPhoto(photo)}
          onDoubleClick={() => onDoubleClickPhoto(photo)}
          onContextMenu={(event) => onContextMenu?.(event, photo)}
        />
      )}
    />
  );
}
