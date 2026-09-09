import { FileInput, FilePenLine, FolderOpen, RefreshCw, type LucideIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import {
  openPhotoDirectoryInFileManager,
  renamePhotoDirectory,
  renamePhotosInDirectoryFromTaxa,
  type PhotoDirectory,
} from "../../api/photos";
import { errorMessage } from "../../api/common";
import { Button, Modal } from "../../shared/ui";
import { waitForOperation } from "../../api/tasks";
import { formatPhotoRenameSummary, photoRenameSummaryFromOperation } from "./photoOperation";

export function DirectoryContextMenu({
  directory,
  x,
  y,
  onClose,
  onRefresh,
  onDirectoryRenamed,
  onStatus,
}: {
  directory: PhotoDirectory;
  x: number;
  y: number;
  onClose: () => void;
  onRefresh: (directory: PhotoDirectory) => Promise<void>;
  onDirectoryRenamed: (directory: PhotoDirectory) => Promise<void> | void;
  onStatus: (message: string, busy?: boolean) => void;
}) {
  const menuRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [renaming, setRenaming] = useState(false);
  const [newName, setNewName] = useState(directory.name);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");

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
    inputRef.current.focus();
    inputRef.current.select();
  }, [renaming]);

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

  async function renameDirectoryPhotos(includeDescendants: boolean) {
    onStatus(includeDescendants ? "Renaming directory photos recursively" : "Renaming directory photos", true);
    const started = await renamePhotosInDirectoryFromTaxa(directory.directory_id, includeDescendants);
    const completed = started.operation.task_id
      ? await waitForOperation(started.operation.task_id)
      : started.operation;
    onStatus(formatPhotoRenameSummary(photoRenameSummaryFromOperation(completed)));
  }

  async function renameDirectoryOnly() {
    const renamed = await renamePhotoDirectory(directory.directory_id, newName);
    onStatus("Folder renamed");
    await onDirectoryRenamed(renamed);
  }

  return (
    <>
      <div
        className="context-menu"
        ref={menuRef}
        role="menu"
        style={{ left: Math.min(x, window.innerWidth - 270), top: Math.min(y, window.innerHeight - 190) }}
      >
        <MenuButton
          icon={RefreshCw}
          label={busy === "Refreshing" ? "Refreshing..." : "Refresh folder"}
          disabled={Boolean(busy)}
          onClick={() => void run("Refreshing", () => onRefresh(directory))}
        />
        <MenuButton
          icon={FilePenLine}
          label="Rename folder"
          disabled={directory.parent_directory_id === null || Boolean(busy)}
          onClick={() => {
            setNewName(directory.name);
            setError("");
            setRenaming(true);
          }}
        />
        <div className="context-separator" role="separator" />
        <MenuButton
          icon={FileInput}
          label={busy === "Renaming files" ? "Renaming..." : "Rename files from taxonomy"}
          disabled={Boolean(busy)}
          onClick={() => void run("Renaming files", () => renameDirectoryPhotos(false))}
        />
        <MenuButton
          icon={FileInput}
          label={busy === "Renaming recursively" ? "Renaming..." : "Rename files recursively from taxonomy"}
          disabled={Boolean(busy)}
          onClick={() => void run("Renaming recursively", () => renameDirectoryPhotos(true))}
        />
        <div className="context-separator" role="separator" />
        <MenuButton
          icon={FolderOpen}
          label={busy === "Opening" ? "Opening..." : "Open in Finder / Explorer"}
          disabled={Boolean(busy)}
          onClick={() => void run("Opening", () => openPhotoDirectoryInFileManager(directory.directory_id))}
        />
        {error && <div className="context-error">{error}</div>}
      </div>

      {renaming && (
        <Modal
          title="Rename folder"
          dismissible={!busy}
          onClose={() => setRenaming(false)}
          actions={
            <>
              <Button disabled={Boolean(busy)} onClick={() => setRenaming(false)}>Cancel</Button>
              <Button
                variant="primary"
                disabled={!newName.trim() || Boolean(busy)}
                onClick={() => void run("Renaming folder", renameDirectoryOnly)}
              >
                {busy === "Renaming folder" ? "Renaming..." : "Rename"}
              </Button>
            </>
          }
        >
          <label className="field-stack">
            <span>Folder name</span>
            <input ref={inputRef} value={newName} onChange={(event) => setNewName(event.target.value)} />
          </label>
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
  onClick,
}: {
  icon: LucideIcon;
  label: string;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button type="button" role="menuitem" disabled={disabled} onClick={onClick}>
      <Icon size={14} />
      <span>{label}</span>
    </button>
  );
}
