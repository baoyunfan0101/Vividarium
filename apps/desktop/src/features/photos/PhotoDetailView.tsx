import { Copy } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import {
  getPhotoMetadata,
  type Photo,
  type PhotoMetadata,
} from "../../api/photos";
import { errorMessage, formatBytes } from "../../api/common";
import { Busy, Button } from "../../shared/ui";
import { PhotoStage } from "./PhotoMedia";
import { useViewState } from "../../shared/viewState";
import { ResizablePanels } from "../../shared/ResizablePanels";
import { usePhotoInteraction, type PhotoOpenHandlers } from "./PhotoInteraction";
import { PhotoPaneHeader } from "./PhotoPaneHeader";
import { usePublishedPhotoTaxonSummary, type PhotoTaxonDisplayState } from "./photoTaxonSummary";

export function PhotoDetailView({
  photo,
  handlers,
  active,
  onPhotoTaxonDisplayState,
  onStatus,
}: {
  photo: Photo;
  handlers: PhotoOpenHandlers;
  active: boolean;
  onPhotoTaxonDisplayState: (state: PhotoTaxonDisplayState | null) => void;
  onStatus: (message: string) => void;
}) {
  const [metadata, setMetadata] = useViewState<PhotoMetadata | null>("photo-detail.metadata", null);
  const [detailScrollTop, setDetailScrollTop] = useViewState("photo-detail.scroll-top", 0);
  const [error, setError] = useState("");
  const [copied, setCopied] = useState("");
  const detailRef = useRef<HTMLDListElement>(null);
  const interaction = usePhotoInteraction({
    photos: [photo],
    handlers,
    stateKey: "photo-detail.interaction",
    onStatus,
  });
  usePublishedPhotoTaxonSummary({
    photoId: photo.photo_id,
    active,
    onChange: onPhotoTaxonDisplayState,
  });

  useEffect(() => {
    let active = true;
    setMetadata(null);
    setError("");
    getPhotoMetadata(photo.photo_id).then((nextMetadata) => {
      if (!active) return;
      setMetadata(nextMetadata);
    }).catch((nextError) => setError(errorMessage(nextError)));
    return () => { active = false; };
  }, [photo.photo_id]);

  useEffect(() => {
    if (detailRef.current) detailRef.current.scrollTop = detailScrollTop;
  }, [metadata]);

  function copy(label: string, value: string) {
    void navigator.clipboard.writeText(value).then(() => {
      setCopied(label);
      window.setTimeout(() => setCopied((current) => current === label ? "" : current), 1200);
    }).catch((nextError) => setError(errorMessage(nextError)));
  }

  return (
    <div className="photo-detail-view">
      <header className="photo-pane-heading">
        <PhotoPaneHeader photo={photo} />
      </header>
      <ResizablePanels
        className="photo-detail-content"
        initialRatio={0.52}
        minFirst={320}
        minSecond={300}
        separatorLabel="Resize photo and details"
        stateKey="photo-detail.columns"
        first={<PhotoStage photo={photo} compact onContextMenu={interaction.openContextMenu} />}
        second={(<div className="photo-detail-sidebar">
          {!metadata && !error ? <Busy label="Loading details" /> : (
          <dl
            className="detail-grid"
            ref={detailRef}
            onScroll={(event) => setDetailScrollTop(event.currentTarget.scrollTop)}
          >
            <DetailValue label="Path" value={photo.relative_path} copied={copied} onCopy={copy} />
            <DetailValue label="Size" value={formatBytes(photo.file_size)} copied={copied} onCopy={copy} />
            <DetailValue label="Captured" value={metadata?.captured_at ?? "-"} copied={copied} onCopy={copy} />
            <DetailValue label="Camera" value={metadata?.camera ?? "-"} copied={copied} onCopy={copy} />
            <DetailValue label="Width" value={formatOptionalNumber(metadata?.width)} copied={copied} onCopy={copy} />
            <DetailValue label="Height" value={formatOptionalNumber(metadata?.height)} copied={copied} onCopy={copy} />
            <DetailValue label="Longitude" value={formatOptionalNumber(metadata?.longitude)} copied={copied} onCopy={copy} />
            <DetailValue label="Latitude" value={formatOptionalNumber(metadata?.latitude)} copied={copied} onCopy={copy} />
            <DetailValue label="EXIF" value={metadata?.exif_json ?? "-"} copied={copied} onCopy={copy} multiline />
          </dl>
        )}
        {error && <div className="inline-error">{error}</div>}
        </div>)}
      />
      {interaction.contextMenu}
    </div>
  );
}

function formatOptionalNumber(value: number | null | undefined): string {
  return value === null || value === undefined ? "-" : String(value);
}

function DetailValue({
  label,
  value,
  copied,
  onCopy,
  multiline = false,
}: {
  label: string;
  value: string;
  copied: string;
  onCopy: (label: string, value: string) => void;
  multiline?: boolean;
}) {
  return (
    <>
      <dt>{label}</dt>
      <dd className="detail-value">
        {multiline ? <pre>{value}</pre> : <span>{value}</span>}
        <Button size="small" title={`Copy ${label}`} onClick={() => onCopy(label, value)}>
          <Copy size={13} />
          <span>{copied === label ? "Copied" : "Copy"}</span>
        </Button>
      </dd>
    </>
  );
}
