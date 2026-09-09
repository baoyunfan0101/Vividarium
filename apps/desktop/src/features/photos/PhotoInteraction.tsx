import { useCallback, useMemo, useState, type MouseEvent } from "react";
import type { Photo } from "../../api/photos";
import { getPhotoMapping, type PhotoMappingSummary } from "../../api/mapping";
import { PhotoContextMenu } from "./PhotoContextMenu";
import { emitPhotoMutation } from "./photoMutations";
import { useViewState } from "../../shared/viewState";

export type PhotoOpenHandlers = {
  openDetails: (photo: Photo) => void;
  openTaxon: (taxonId: number) => void;
  openMappingEditor: (photo: Photo) => void;
  openFullscreen: (photo: Photo, onReturnFocus?: () => void) => void;
};

type PhotoContextState = {
  photo: Photo;
  mapping: PhotoMappingSummary | null;
  loading: boolean;
  x: number;
  y: number;
};

export function usePhotoInteraction({
  photos,
  handlers,
  knownMapping,
  selectFirst = true,
  stateKey,
  onStatus,
}: {
  photos: Photo[];
  handlers: PhotoOpenHandlers;
  knownMapping?: (photo: Photo) => PhotoMappingSummary | null | undefined;
  selectFirst?: boolean;
  stateKey?: string;
  onStatus: (message: string) => void;
}) {
  const [selectedId, setSelectedId] = useViewState<number | null>(
    stateKey ? `${stateKey}.selected-photo` : null,
    null,
  );
  const [context, setContext] = useState<PhotoContextState | null>(null);
  const selected = useMemo(
    () => photos.find((photo) => photo.photo_id === selectedId) ?? (selectFirst ? photos[0] : null) ?? null,
    [photos, selectFirst, selectedId],
  );
  const selectPhoto = useCallback((photo: Photo) => setSelectedId(photo.photo_id), []);
  const clearSelection = useCallback(() => setSelectedId(null), []);

  const openContextMenu = useCallback((event: MouseEvent, photo: Photo) => {
    event.preventDefault();
    selectPhoto(photo);
    const mapping = knownMapping?.(photo);
    const loading = mapping === undefined;
    setContext({ photo, mapping: mapping ?? null, loading, x: event.clientX, y: event.clientY });
    if (!loading) return;
    void getPhotoMapping(photo.photo_id)
      .then((nextMapping) => setContext((current) => (
        current?.photo.photo_id === photo.photo_id
          ? { ...current, mapping: nextMapping, loading: false }
          : current
      )))
      .catch(() => setContext((current) => (
        current?.photo.photo_id === photo.photo_id
          ? { ...current, loading: false }
          : current
      )));
  }, [knownMapping, selectPhoto]);

  const contextMenu = context ? (
    <PhotoContextMenu
      {...context}
      onClose={() => setContext(null)}
      onChanged={(photo) => {
        selectPhoto(photo);
        emitPhotoMutation({ photoId: photo.photo_id, kind: "photo", photo });
      }}
      onMappingChanged={() => emitPhotoMutation({ photoId: context.photo.photo_id, kind: "mapping" })}
      onStatus={onStatus}
      onOpenDetails={() => handlers.openDetails(context.photo)}
      onOpenFullscreen={() => handlers.openFullscreen(context.photo)}
      onOpenTaxon={handlers.openTaxon}
      onOpenMappingEditor={() => handlers.openMappingEditor(context.photo)}
    />
  ) : null;

  return {
    selected,
    selectedId: selected?.photo_id ?? null,
    clearSelection,
    selectPhoto,
    openContextMenu,
    contextMenu,
  };
}
