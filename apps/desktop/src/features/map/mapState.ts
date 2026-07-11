export type MapPreviewMode = "image" | "details";

export type CachedMapState = {
  center: [number, number] | null;
  zoom: number | null;
  selectedPhotoId: number | null;
  previewPhotoId: number | null;
  previewMode: MapPreviewMode;
};

export const MAP_STATE_KEY = "phytoindex.map.state";

export const DEFAULT_MAP_STATE: CachedMapState = {
  center: null,
  zoom: null,
  selectedPhotoId: null,
  previewPhotoId: null,
  previewMode: "image",
};
