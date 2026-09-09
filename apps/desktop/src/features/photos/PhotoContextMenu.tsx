import {
  Database,
  FileInput,
  FilePenLine,
  FolderOpen,
  Info,
  Link,
  Link2,
  Maximize,
} from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  renamePhoto,
  renamePhotoFromTaxon,
  revealPhotoInFileManager,
  type Photo,
} from "../../api/photos";
import { errorMessage } from "../../api/common";
import { remapPhoto, type PhotoMappingSummary } from "../../api/mapping";
import { Button, Modal } from "../../shared/ui";
import { MappingBadge } from "../mapping/MappingBadge";
import { renameFromTaxonomyStatus } from "./photoRenameStatus";

export function PhotoContextMenu({
  photo,
  mapping,
  loading,
  x,
  y,
  onClose,
  onChanged,
  onMappingChanged,
  onOpenDetails,
  onOpenFullscreen,
  onOpenTaxon,
  onOpenMappingEditor,
  onStatus,
}: {
  photo: Photo;
  mapping: PhotoMappingSummary | null;
  loading: boolean;
  x: number;
  y: number;
  onClose: () => void;
  onChanged: (photo: Photo) => void;
  onMappingChanged: () => void;
  onOpenDetails: () => void;
  onOpenFullscreen: () => void;
  onOpenTaxon: (taxonId: number) => void;
  onOpenMappingEditor: () => void;
  onStatus: (message: string) => void;
}) {
  const [renaming, setRenaming] = useState(false);
  const [newFilename, setNewFilename] = useState(photo.filename);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const menuRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (renaming || busy) return;
    const close = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) onClose();
    };
    const closeKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", closeKey);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", closeKey);
    };
  }, [busy, onClose, renaming]);

  useEffect(() => {
    if (!renaming || !inputRef.current) return;
    const dot = newFilename.lastIndexOf(".");
    inputRef.current.focus();
    inputRef.current.setSelectionRange(0, dot > 0 ? dot : newFilename.length);
  }, [renaming]);

  const matched = mapping?.status === "matched" && mapping.taxon_id !== null;

  async function run(label: string, action: () => Promise<void>) {
    setBusy(label);
    setError("");
    try {
      await action();
      onClose();
    } catch (nextError) {
      setError(errorMessage(nextError));
      setBusy("");
    }
  }

  return (
    <>
      <div
        className="context-menu"
        ref={menuRef}
        style={{ left: Math.min(x, window.innerWidth - 268), top: Math.min(y, window.innerHeight - 330) }}
        role="menu"
      >
        <MenuButton icon={Maximize} label="View fullscreen" onClick={() => {
          onOpenFullscreen();
          onClose();
        }} />
        <MenuButton icon={Info} label="View photo details" onClick={onOpenDetails} />
        <MenuSeparator />
        <MenuButton
          icon={Database}
          label="View taxon details"
          disabled={!matched}
          trailing={mapping ? <MappingBadge status={mapping.status} /> : <span className="context-loading">{loading ? "Loading" : "Unavailable"}</span>}
          onClick={() => matched && onOpenTaxon(mapping.taxon_id!)}
        />
        <MenuButton icon={Link} label="Edit mapping" onClick={onOpenMappingEditor} />
        <MenuButton
          icon={Link2}
          label="Remap from filename"
          disabled={Boolean(busy)}
          onClick={() => void run("Remapping", async () => {
            await remapPhoto(photo.photo_id);
            onMappingChanged();
          })}
        />
        <MenuSeparator />
        <MenuButton icon={FilePenLine} label="Rename" disabled={Boolean(busy)} onClick={() => setRenaming(true)} />
        <MenuButton
          icon={FileInput}
          label="Rename from taxonomy"
          disabled={!matched || Boolean(busy)}
          onClick={() => void run("Renaming", async () => {
            const renamed = await renamePhotoFromTaxon(photo.photo_id);
            onChanged(renamed);
            onStatus(renameFromTaxonomyStatus(photo.filename, renamed.filename));
          })}
        />
        <MenuSeparator />
        <MenuButton
          icon={FolderOpen}
          label="Reveal in Finder / Explorer"
          disabled={Boolean(busy)}
          onClick={() => void run("Revealing", () => revealPhotoInFileManager(photo.photo_id))}
        />
        {error && <div className="context-error">{error}</div>}
      </div>

      {renaming && (
        <Modal
          title="Rename photo"
          dismissible={!busy}
          onClose={() => setRenaming(false)}
          actions={
            <>
              <Button disabled={Boolean(busy)} onClick={() => setRenaming(false)}>Cancel</Button>
              <Button
                variant="primary"
                disabled={!newFilename.trim() || Boolean(busy)}
                onClick={() => void run("Renaming", async () => onChanged(await renamePhoto(photo.photo_id, newFilename.trim())))}
              >
                {busy === "Renaming" ? "Renaming..." : "Rename"}
              </Button>
            </>
          }
        >
          <label className="field-stack">
            <span>Filename</span>
            <input ref={inputRef} value={newFilename} onChange={(event) => setNewFilename(event.target.value)} />
          </label>
          <span className="field-hint">The extension is part of the filename.</span>
          {error && <div className="inline-error">{error}</div>}
        </Modal>
      )}
    </>
  );
}

function MenuButton({
  icon: Icon,
  label,
  disabled,
  trailing,
  onClick,
}: {
  icon: typeof Info;
  label: string;
  disabled?: boolean;
  trailing?: ReactNode;
  onClick: () => void;
}) {
  return (
    <button type="button" role="menuitem" disabled={disabled} onClick={onClick}>
      <Icon size={14} />
      <span className="context-menu-label">{label}</span>
      {trailing && <span className="context-menu-trailing">{trailing}</span>}
    </button>
  );
}

function MenuSeparator() {
  return <div className="context-separator" role="separator" />;
}
