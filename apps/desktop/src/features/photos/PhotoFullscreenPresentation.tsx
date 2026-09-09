import { useEffect, useRef } from "react";
import type { Photo } from "../../api/photos";
import { PhotoStage } from "./PhotoMedia";

export function PhotoFullscreenPresentation({
  photo,
  onExit,
}: {
  photo: Photo;
  onExit: () => void;
}) {
  const presentationRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    presentationRef.current?.focus({ preventScroll: true });
  }, []);

  useEffect(() => {
    const exitOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      void onExit();
    };
    window.addEventListener("keydown", exitOnEscape, true);
    return () => window.removeEventListener("keydown", exitOnEscape, true);
  }, [onExit]);

  return (
    <div ref={presentationRef} className="photo-fullscreen-presentation" tabIndex={-1}>
      <PhotoStage photo={photo} />
    </div>
  );
}
