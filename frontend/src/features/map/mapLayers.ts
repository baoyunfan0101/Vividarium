import maplibregl, { type Map as MapLibreMap } from "maplibre-gl";
import type { MapPhoto } from "../../api";
import { writeStorage } from "../../lib/storage";
import { MAP_STATE_KEY, type MapPreviewMode } from "./mapState";
import type { MapPhotoFeatureCollection } from "./mapUtils";

const PHOTO_SOURCE_ID = "map-photos";

export function syncPhotoLayers(
  map: MapLibreMap,
  data: MapPhotoFeatureCollection,
  photos: MapPhoto[],
  shouldFitBounds: boolean,
): void {
  addPhotoLayers(map, data);
  const source = map.getSource(PHOTO_SOURCE_ID);
  if (source && "setData" in source) {
    (source as maplibregl.GeoJSONSource).setData(data);
  }
  if (shouldFitBounds) {
    fitPhotoBounds(map, photos);
  }
}

export function persistMapView(
  map: MapLibreMap,
  selectedPhotoId: number | null,
  previewPhotoId: number | null,
  previewMode: MapPreviewMode,
): void {
  const center = map.getCenter();
  writeStorage(MAP_STATE_KEY, {
    center: [center.lng, center.lat],
    zoom: map.getZoom(),
    selectedPhotoId,
    previewPhotoId,
    previewMode,
  });
}

export function expandCluster(map: MapLibreMap, clusterId: number, coordinates: [number, number]): void {
  const source = map.getSource(PHOTO_SOURCE_ID);
  if (!source || !("getClusterExpansionZoom" in source)) {
    return;
  }
  const getZoom = (source as unknown as {
    getClusterExpansionZoom: (
      id: number,
      callback?: (error: Error | null, zoom: number) => void,
    ) => Promise<number> | void;
  }).getClusterExpansionZoom;

  const result = getZoom.call(source, clusterId, (error, zoom) => {
    if (!error) {
      map.easeTo({ center: coordinates, zoom });
    }
  });
  if (result instanceof Promise) {
    result.then((zoom) => map.easeTo({ center: coordinates, zoom }));
  }
}

function addPhotoLayers(map: MapLibreMap, data: MapPhotoFeatureCollection): void {
  if (map.getSource(PHOTO_SOURCE_ID)) {
    return;
  }
  map.addSource(PHOTO_SOURCE_ID, {
    type: "geojson",
    data,
    cluster: true,
    clusterRadius: 52,
    clusterMaxZoom: 14,
  });

  map.addLayer({
    id: "photo-clusters",
    type: "circle",
    source: PHOTO_SOURCE_ID,
    filter: ["has", "point_count"],
    paint: {
      "circle-color": ["step", ["get", "point_count"], "#4f8f7b", 20, "#2f6d5d", 80, "#174c42"],
      "circle-radius": ["step", ["get", "point_count"], 18, 20, 24, 80, 32],
      "circle-stroke-color": "#ffffff",
      "circle-stroke-width": 2,
    },
  });

  map.addLayer({
    id: "photo-cluster-counts",
    type: "symbol",
    source: PHOTO_SOURCE_ID,
    filter: ["has", "point_count"],
    layout: {
      "text-field": ["get", "point_count_abbreviated"],
      "text-size": 12,
    },
    paint: {
      "text-color": "#ffffff",
    },
  });

  map.addLayer({
    id: "photo-points",
    type: "circle",
    source: PHOTO_SOURCE_ID,
    filter: ["!", ["has", "point_count"]],
    paint: {
      "circle-color": "#f7f9f8",
      "circle-radius": 7,
      "circle-stroke-color": "#245f4e",
      "circle-stroke-width": 3,
    },
  });
}

function fitPhotoBounds(map: MapLibreMap, photos: MapPhoto[]): void {
  if (!photos.length) {
    return;
  }
  const bounds = new maplibregl.LngLatBounds();
  for (const photo of photos) {
    if (photo.longitude !== null && photo.latitude !== null) {
      bounds.extend([photo.longitude, photo.latitude]);
    }
  }
  if (!bounds.isEmpty()) {
    map.fitBounds(bounds, {
      padding: 70,
      maxZoom: photos.length === 1 ? 12 : 11,
      duration: 0,
    });
  }
}
