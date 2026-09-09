import { useEffect, useRef, useState } from "react";
import {
  getPhotoMapping,
  getPhotoTaxonDisplaySummary,
  type PhotoTaxonStatus,
} from "../../api/mapping";
import type { TaxonDisplaySummary } from "../../api/taxonomy";
import type { PhotoTaxonDisplayState } from "./photoTaxonDisplayState";
import {
  loadSelectedPhotoMappingStatus,
  PhotoMappingStatusCache,
} from "./photoMappingStatusCache";
import { useTaxonomyMutation } from "../taxonomy/taxonomyMutations";
import { usePhotoMutation } from "./photoMutations";
import {
  loadSelectedPhotoTaxonSummary,
  PhotoTaxonSummaryCache,
} from "./photoTaxonSummaryCache";

export type { PhotoTaxonDisplayState } from "./photoTaxonDisplayState";

export function usePhotoTaxonDisplaySummary(photoId: number | null): TaxonDisplaySummary | null {
  const cacheRef = useRef<PhotoTaxonSummaryCache | null>(null);
  if (cacheRef.current === null) {
    cacheRef.current = new PhotoTaxonSummaryCache(getPhotoTaxonDisplaySummary);
  }
  const [loaded, setLoaded] = useState<{
    key: string;
    summary: TaxonDisplaySummary | null;
  }>({ key: "", summary: null });
  const [revision, setRevision] = useState(0);
  const request = useRef(0);
  const selectionKey = `${photoId ?? "none"}:${revision}`;

  usePhotoMutation((mutation) => {
    if (mutation.kind !== "mapping") return;
    const affected = mutation.photoId === null
      || mutation.photoId === photoId
      || mutation.photoIds?.includes(photoId ?? -1);
    if (mutation.photoId === null) cacheRef.current!.invalidate();
    else {
      cacheRef.current!.invalidate(mutation.photoId);
      mutation.photoIds?.forEach((id) => cacheRef.current!.invalidate(id));
    }
    if (affected) setRevision((current) => current + 1);
  });
  useTaxonomyMutation(() => {
    cacheRef.current!.invalidate();
    if (photoId !== null) setRevision((current) => current + 1);
  });

  useEffect(() => {
    const current = ++request.current;
    let active = true;
    void loadSelectedPhotoTaxonSummary(photoId, cacheRef.current!).then((next) => {
      if (active && current === request.current) setLoaded({ key: selectionKey, summary: next });
    }).catch(() => {
      if (active && current === request.current) setLoaded({ key: selectionKey, summary: null });
    });
    return () => {
      active = false;
    };
  }, [photoId, revision, selectionKey]);

  return loaded.key === selectionKey ? loaded.summary : null;
}

export function usePublishedPhotoTaxonSummary({
  photoId,
  active,
  onChange,
}: {
  photoId: number | null;
  active: boolean;
  onChange: (state: PhotoTaxonDisplayState | null) => void;
}): void {
  const summary = usePhotoTaxonDisplaySummary(active ? photoId : null);
  const mappingStatus = usePhotoMappingStatus(active ? photoId : null);
  useEffect(() => {
    onChange(summary || mappingStatus ? { summary, mappingStatus } : null);
  }, [mappingStatus, onChange, summary]);
  useEffect(() => () => onChange(null), [onChange]);
}

function usePhotoMappingStatus(photoId: number | null): PhotoTaxonStatus | null {
  const cacheRef = useRef<PhotoMappingStatusCache | null>(null);
  if (cacheRef.current === null) {
    cacheRef.current = new PhotoMappingStatusCache(async (id) => (await getPhotoMapping(id)).status);
  }
  const [loaded, setLoaded] = useState<{ key: string; status: PhotoTaxonStatus | null }>({ key: "", status: null });
  const [revision, setRevision] = useState(0);
  const request = useRef(0);
  const selectionKey = `${photoId ?? "none"}:${revision}`;

  usePhotoMutation((mutation) => {
    if (mutation.kind !== "mapping") return;
    const affected = mutation.photoId === null
      || mutation.photoId === photoId
      || mutation.photoIds?.includes(photoId ?? -1);
    if (mutation.photoId === null) cacheRef.current!.invalidate();
    else {
      cacheRef.current!.invalidate(mutation.photoId);
      mutation.photoIds?.forEach((id) => cacheRef.current!.invalidate(id));
    }
    if (affected) setRevision((current) => current + 1);
  });

  useEffect(() => {
    const current = ++request.current;
    let active = true;
    const load = loadSelectedPhotoMappingStatus(photoId, cacheRef.current!);
    void load.then((status) => {
      if (active && current === request.current) setLoaded({ key: selectionKey, status });
    }).catch(() => {
      if (active && current === request.current) setLoaded({ key: selectionKey, status: null });
    });
    return () => {
      active = false;
    };
  }, [photoId, revision, selectionKey]);

  return loaded.key === selectionKey ? loaded.status : null;
}
