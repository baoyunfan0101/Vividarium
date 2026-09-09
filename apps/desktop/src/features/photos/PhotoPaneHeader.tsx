import type { Photo } from "../../api/photos";
import { photoPaneHeaderLines } from "./photoFormatting";

export function PhotoPaneHeader({ photo }: { photo: Photo }) {
  const lines = photoPaneHeaderLines(photo);
  return (
    <div className="photo-pane-header">
      <strong>{lines.filename}</strong>
      <span>{lines.summary}</span>
    </div>
  );
}
