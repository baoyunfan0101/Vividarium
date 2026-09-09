import type { Photo } from "../../api/photos";

type PhotoActivationScheduler = {
  setTimeout: (callback: () => void, delay: number) => number;
  clearTimeout: (timer: number) => void;
};

type PhotoActivationCallbacks = {
  onSelect: (photo: Photo) => void;
  onOpenImage: (photo: Photo) => void;
  onOpenFullscreen: (photo: Photo) => void;
};

const photoDoubleClickDelayMs = 250;

export function createPhotoActivationController(
  callbacks: PhotoActivationCallbacks,
  scheduler: PhotoActivationScheduler = {
    setTimeout: (callback, delay) => window.setTimeout(callback, delay),
    clearTimeout: (timer) => window.clearTimeout(timer),
  },
) {
  let singleClickTimer: number | null = null;

  const cancelPendingClick = () => {
    if (singleClickTimer === null) return;
    scheduler.clearTimeout(singleClickTimer);
    singleClickTimer = null;
  };

  const clickPhoto = (photo: Photo) => {
    callbacks.onSelect(photo);
    cancelPendingClick();
    singleClickTimer = scheduler.setTimeout(() => {
      singleClickTimer = null;
      callbacks.onOpenImage(photo);
    }, photoDoubleClickDelayMs);
  };

  const doubleClickPhoto = (photo: Photo) => {
    cancelPendingClick();
    callbacks.onSelect(photo);
    callbacks.onOpenFullscreen(photo);
  };

  return {
    clickPhoto,
    doubleClickPhoto,
    cancelPendingClick,
    dispose: cancelPendingClick,
  };
}
