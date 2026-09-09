import {
  DatabaseBackup,
  FolderInput,
  FolderOpen,
  FolderSync,
  Pencil,
  RefreshCcw,
  Trash2,
} from "lucide-react";
import { useEffect, useState, type MouseEvent } from "react";
import { errorMessage } from "../../api/common";
import { selectDatabaseDestination, selectPhotoDirectory, selectSqliteDatabase } from "../../api/dialogs";
import type { OperationState } from "../../api/tasks";
import {
  listPhotoLibraries,
  openPathInFileManager,
  photoLibraryAvailabilityLabel,
  rebindPhotoLibraryDatabase,
  rebindPhotoLibraryRoot,
  relocatePhotoLibraryDatabase,
  removePhotoLibrary,
  renamePhotoLibrary,
  switchPhotoLibrary,
  type PhotoLibraryWorkspace,
} from "../../api/storage";
import { Button, SectionHeader } from "../../shared/ui";
import { shouldSwitchPhotoLibrary } from "./photoLibraryUx";

export function PhotoLibrariesSettings({
  onChanged,
  onOpenPhotoLibrary,
  onPhotoOperationStarted,
  blockingOperation,
  operationError,
}: {
  onChanged?: (resetPhotoTabs: boolean) => Promise<void>;
  onOpenPhotoLibrary: () => Promise<boolean>;
  onPhotoOperationStarted: (operation: OperationState | null) => void;
  blockingOperation: OperationState | null;
  operationError: string;
}) {
  const [libraries, setLibraries] = useState<PhotoLibraryWorkspace[]>([]);
  const [busy, setBusy] = useState("");
  const [busyLibraryUuid, setBusyLibraryUuid] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const mutationLocked = Boolean(
    blockingOperation && ["queued", "running"].includes(blockingOperation.state),
  );
  const mutationLockTitle = mutationLocked
    ? "Unavailable while Photo Library work is running"
    : undefined;

  async function load() {
    try {
      setLibraries(await listPhotoLibraries());
      setMessage("");
    } catch (nextError) {
      setMessage(errorMessage(nextError));
    }
  }

  useEffect(() => { void load(); }, []);

  async function openPhotoLibrary() {
    setBusy("Opening Photo Library");
    setBusyLibraryUuid(null);
    setMessage("");
    try {
      if (await onOpenPhotoLibrary()) await load();
    } finally {
      setBusy("");
      setBusyLibraryUuid(null);
    }
  }

  async function mutate(
    label: string,
    action: () => Promise<unknown>,
    resetPhotoTabs = false,
    libraryUuid: string | null = null,
  ) {
    setBusy(label);
    setBusyLibraryUuid(libraryUuid);
    setMessage("");
    try {
      await action();
      await load();
      await onChanged?.(resetPhotoTabs);
    } catch (nextError) {
      setMessage(errorMessage(nextError));
    } finally {
      setBusy("");
      setBusyLibraryUuid(null);
    }
  }

  function switchLibrary(library: PhotoLibraryWorkspace) {
    const available = library.root_available && library.database_available;
    if (!shouldSwitchPhotoLibrary(library.active, available, Boolean(busy) || mutationLocked)) return;
    void mutate(
      "Switching Photo Library",
      async () => {
        const activation = await switchPhotoLibrary(library.library_uuid);
        onPhotoOperationStarted(activation.operation);
      },
      true,
      library.library_uuid,
    );
  }

  function retryLibrary(library: PhotoLibraryWorkspace) {
    if (busy || mutationLocked) return;
    void mutate(
      "Retrying Photo Library",
      async () => {
        const activation = await switchPhotoLibrary(library.library_uuid);
        onPhotoOperationStarted(activation.operation);
      },
      true,
      library.library_uuid,
    );
  }

  function stopCardActivation(event: MouseEvent<HTMLElement>) {
    event.stopPropagation();
  }

  return (
    <div className="settings-section">
      <SectionHeader
        title="Photo Libraries"
        detail="Open and manage registered photo libraries. Select a card to make that library active."
        actions={(
          <>
            <Button disabled={Boolean(busy)} onClick={() => void load()}><RefreshCcw size={13} />Refresh</Button>
            <Button
              variant="primary"
              disabled={Boolean(busy) || mutationLocked}
              title={mutationLockTitle}
              onClick={() => void openPhotoLibrary()}
            >
              <FolderOpen size={13} />{busy === "Opening Photo Library" ? "Opening..." : "Open Photo Library"}
            </Button>
          </>
        )}
      />
      <div className="library-settings-list" aria-busy={Boolean(busy)}>
        {libraries.map((library) => (
          <article
            aria-busy={busyLibraryUuid === library.library_uuid}
            className={`library-settings-row${library.active ? " active" : ""}${busy ? " busy" : ""}${mutationLocked ? " locked" : ""}${!library.root_available || !library.database_available ? " unavailable" : ""}`}
            key={library.library_uuid}
          >
            <button
              aria-current={library.active ? "true" : undefined}
              aria-label={`${library.display_name}${library.active ? ", active Photo Library" : ", select Photo Library"}`}
              className="library-card-select"
              disabled={Boolean(busy) || mutationLocked || library.active || !library.root_available || !library.database_available}
              title={mutationLockTitle}
              type="button"
              onClick={() => switchLibrary(library)}
            />
            <div className="library-heading">
              <strong>{library.display_name}</strong>
              <span className={library.root_available && library.database_available ? "available" : "unavailable"}>
                {photoLibraryAvailabilityLabel(library)}
              </span>
              {busyLibraryUuid === library.library_uuid && busy === "Switching Photo Library"
                ? <b>Switching...</b>
                : library.active && <b>Active</b>}
            </div>
            <code>{library.root_path}</code>
            <code>{library.db_path}</code>
            <small>Last opened: {library.last_opened_at}</small>
            <div className="library-actions" onClick={stopCardActivation} onKeyDown={(event) => event.stopPropagation()}>
              {library.active && operationError && (
                <Button
                  size="small"
                  disabled={Boolean(busy) || mutationLocked}
                  onClick={() => retryLibrary(library)}
                >
                  <RefreshCcw size={12} />{busyLibraryUuid === library.library_uuid && busy === "Retrying Photo Library" ? "Retrying..." : "Retry"}
                </Button>
              )}
              <Button
                size="small"
                disabled={Boolean(busy) || !library.root_available}
                onClick={() => void mutate("Opening photo root", () => openPathInFileManager(library.root_path), false, library.library_uuid)}
              >
                <FolderOpen size={12} />{busyLibraryUuid === library.library_uuid && busy === "Opening photo root" ? "Opening..." : "Open"}
              </Button>
              <Button size="small" disabled={Boolean(busy)} onClick={() => {
                const name = window.prompt("Photo Library name", library.display_name)?.trim();
                if (name) void mutate("Renaming Photo Library", () => renamePhotoLibrary(library.library_uuid, name), false, library.library_uuid);
              }}><Pencil size={12} />{busyLibraryUuid === library.library_uuid && busy === "Renaming Photo Library" ? "Renaming..." : "Rename"}</Button>
              <Button size="small" disabled={Boolean(busy) || mutationLocked} title={mutationLockTitle} onClick={() => void selectPhotoDirectory(library.root_path).then((path) => {
                if (path) return mutate("Rebinding photo root", async () => {
                  const activation = await rebindPhotoLibraryRoot(library.library_uuid, path);
                  onPhotoOperationStarted(activation.operation);
                }, library.active, library.library_uuid);
              })}><FolderSync size={12} />{busyLibraryUuid === library.library_uuid && busy === "Rebinding photo root" ? "Rebinding..." : "Rebind Root"}</Button>
              <Button size="small" disabled={Boolean(busy) || mutationLocked} title={mutationLockTitle} onClick={() => void selectSqliteDatabase(library.db_path).then((path) => {
                if (path) return mutate("Rebinding Photo Library database", async () => {
                  const activation = await rebindPhotoLibraryDatabase(library.library_uuid, path);
                  onPhotoOperationStarted(activation.operation);
                }, library.active, library.library_uuid);
              })}><DatabaseBackup size={12} />{busyLibraryUuid === library.library_uuid && busy === "Rebinding Photo Library database" ? "Rebinding..." : "Rebind DB"}</Button>
              <Button size="small" disabled={Boolean(busy) || mutationLocked || !library.database_available} title={mutationLockTitle} onClick={() => void selectDatabaseDestination(library.db_path).then((path) => {
                if (path) return mutate("Moving Photo Library database", () => relocatePhotoLibraryDatabase(library.library_uuid, path), library.active, library.library_uuid);
              })}><FolderInput size={12} />{busyLibraryUuid === library.library_uuid && busy === "Moving Photo Library database" ? "Moving..." : "Move DB"}</Button>
              <Button
                size="small"
                disabled={Boolean(busy) || mutationLocked}
                title={mutationLockTitle}
                onClick={() => void mutate(
                  "Removing Photo Library registration",
                  () => removePhotoLibrary(library.library_uuid),
                  library.active,
                  library.library_uuid,
                )}
              >
                <Trash2 size={12} />{busyLibraryUuid === library.library_uuid && busy === "Removing Photo Library registration" ? "Removing..." : "Remove"}
              </Button>
            </div>
          </article>
        ))}
      </div>
      {(message || operationError) && <div className="inline-error">{message || operationError}</div>}
    </div>
  );
}
