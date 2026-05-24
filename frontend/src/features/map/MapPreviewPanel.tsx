import { Image, Info, X } from "lucide-react";
import type { MapPhoto } from "../../api";
import { PhotoPreview } from "../../components/photo";
import type { MapPreviewMode } from "./mapState";

type MapPreviewPanelProps = {
  photo: MapPhoto;
  mode: MapPreviewMode;
  onClose: () => void;
  onModeChange: (mode: MapPreviewMode) => void;
};

export function MapPreviewPanel({ photo, mode, onClose, onModeChange }: MapPreviewPanelProps) {
  return (
    <div className="map-preview-panel">
      <div className="map-preview-toolbar">
        <button className="map-window-dot close" type="button" title="Close preview" onClick={onClose}>
          <X size={9} />
        </button>
        <button
          className={mode === "details" ? "map-window-dot active" : "map-window-dot"}
          type="button"
          title="Details"
          onClick={() => onModeChange("details")}
        >
          <Info size={10} />
        </button>
        <button
          className={mode === "image" ? "map-window-dot active" : "map-window-dot"}
          type="button"
          title="Image"
          onClick={() => onModeChange("image")}
        >
          <Image size={10} />
        </button>
      </div>
      <PhotoPreview photo={photo} mode={mode} />
    </div>
  );
}
