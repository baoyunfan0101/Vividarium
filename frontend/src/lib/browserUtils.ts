import type { Photo } from "../api";

export function isSelectionKey(event: KeyboardEvent): boolean {
  return event.key === "ArrowDown" || event.key === "ArrowUp";
}

export function isFormElement(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  return ["INPUT", "SELECT", "TEXTAREA"].includes(target.tagName) || target.isContentEditable;
}

export function nextPhotoSelection(photos: Photo[], current: Photo | null, direction: 1 | -1): Photo | null {
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

export function togglePhotoSelection(current: Photo | null, next: Photo): Photo | null {
  return current?.photo_id === next.photo_id ? null : next;
}

export function shouldClearSelection(event: React.MouseEvent<HTMLElement>): boolean {
  const target = event.target;
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  return !target.closest("button, input, select, textarea, a");
}

export function blurActiveElement() {
  if (document.activeElement instanceof HTMLElement) {
    document.activeElement.blur();
  }
}
