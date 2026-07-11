import "maplibre-gl/dist/maplibre-gl.css";

import { useEffect, useMemo, useRef, useState } from "react";
import maplibregl, { type Map as MapLibreMap } from "maplibre-gl";
import { getMapPhotos, photoThumbnailUrl, type MapPhoto } from "../../bridge";
import { readStorage, writeStorage } from "../../lib/storage";
import { MapPreviewPanel } from "./MapPreviewPanel";
import { expandCluster, persistMapView, syncPhotoLayers } from "./mapLayers";
import { getMapTileProvider, MAP_PROVIDER_KEY } from "./mapProviders";
import { DEFAULT_MAP_STATE, MAP_STATE_KEY, type MapPreviewMode } from "./mapState";
import { centerForPhotos, mapPhotosToGeoJson, type MapPhotoFeatureCollection } from "./mapUtils";

export function MapPage() {
  const cachedState = readStorage(MAP_STATE_KEY, DEFAULT_MAP_STATE);
  const containerRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<MapLibreMap | null>(null);
  const geoJsonRef = useRef<MapPhotoFeatureCollection>(mapPhotosToGeoJson([]));
  const photosRef = useRef<MapPhoto[]>([]);
  const selectedPhotoIdRef = useRef<number | null>(cachedState.selectedPhotoId);
  const previewPhotoIdRef = useRef<number | null>(cachedState.previewPhotoId);
  const previewModeRef = useRef<MapPreviewMode>(cachedState.previewMode);
  const [photos, setPhotos] = useState<MapPhoto[]>([]);
  const [selectedPhotoId, setSelectedPhotoId] = useState<number | null>(cachedState.selectedPhotoId);
  const [previewPhotoId, setPreviewPhotoId] = useState<number | null>(cachedState.previewPhotoId);
  const [previewMode, setPreviewMode] = useState<MapPreviewMode>(cachedState.previewMode);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [providerId] = useState(() => readStorage(MAP_PROVIDER_KEY, "osm"));
  const provider = getMapTileProvider(providerId);
  const selectedPhoto = photos.find((photo) => photo.photo_id === selectedPhotoId) ?? null;
  const previewPhoto = photos.find((photo) => photo.photo_id === previewPhotoId) ?? null;
  const geoJson = useMemo(() => mapPhotosToGeoJson(photos), [photos]);

  useEffect(() => {
    geoJsonRef.current = geoJson;
    photosRef.current = photos;
  }, [geoJson, photos]);

  useEffect(() => {
    selectedPhotoIdRef.current = selectedPhotoId;
    previewPhotoIdRef.current = previewPhotoId;
    previewModeRef.current = previewMode;
  }, [selectedPhotoId, previewPhotoId, previewMode]);

  useEffect(() => {
    setLoading(true);
    getMapPhotos()
      .then((items) => {
        setPhotos(items);
        setError(null);
      })
      .catch((nextError) => setError(nextError instanceof Error ? nextError.message : String(nextError)))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    writeStorage(MAP_PROVIDER_KEY, providerId);
  }, [providerId]);

  useEffect(() => {
    const map = mapRef.current;
    const center = map?.getCenter();
    writeStorage(MAP_STATE_KEY, {
      center: center ? [center.lng, center.lat] : cachedState.center,
      zoom: map?.getZoom() ?? cachedState.zoom,
      selectedPhotoId,
      previewPhotoId,
      previewMode,
    });
  }, [selectedPhotoId, previewPhotoId, previewMode]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }

    const map = new maplibregl.Map({
      container,
      style: provider.style,
      center: cachedState.center ?? centerForPhotos(photos),
      zoom: cachedState.zoom ?? (photos.length === 1 ? 11 : 4),
    });
    mapRef.current = map;
    map.addControl(new maplibregl.NavigationControl({ visualizePitch: true }), "top-right");

    map.on("load", () => {
      syncPhotoLayers(map, geoJsonRef.current, photosRef.current, !cachedState.center);
    });
    map.on("moveend", () => persistMapView(map, selectedPhotoIdRef.current, previewPhotoIdRef.current, previewModeRef.current));
    map.on("zoomend", () => persistMapView(map, selectedPhotoIdRef.current, previewPhotoIdRef.current, previewModeRef.current));

    map.on("click", "photo-clusters", (event) => {
      const feature = map.queryRenderedFeatures(event.point, { layers: ["photo-clusters"] })[0];
      const clusterId = feature?.properties?.cluster_id;
      const geometry = feature?.geometry;
      if (clusterId === undefined || geometry.type !== "Point") {
        return;
      }
      expandCluster(map, Number(clusterId), geometry.coordinates as [number, number]);
    });

    map.on("click", "photo-points", (event) => {
      const feature = map.queryRenderedFeatures(event.point, { layers: ["photo-points"] })[0];
      const photoId = Number(feature?.properties?.photo_id);
      if (Number.isFinite(photoId)) {
        setSelectedPhotoId(photoId);
      }
    });

    map.on("click", (event) => {
      const features = map.queryRenderedFeatures(event.point, {
        layers: ["photo-clusters", "photo-points"],
      });
      if (!features.length) {
        setSelectedPhotoId(null);
        setPreviewPhotoId(null);
      }
    });

    map.on("mouseenter", "photo-clusters", () => { map.getCanvas().style.cursor = "pointer"; });
    map.on("mouseleave", "photo-clusters", () => { map.getCanvas().style.cursor = ""; });
    map.on("mouseenter", "photo-points", () => { map.getCanvas().style.cursor = "pointer"; });
    map.on("mouseleave", "photo-points", () => { map.getCanvas().style.cursor = ""; });

    return () => {
      map.remove();
      mapRef.current = null;
    };
  }, [provider.id]);

  useEffect(() => {
    const map = mapRef.current;
    if (!map || !map.isStyleLoaded()) {
      return;
    }
    syncPhotoLayers(map, geoJson, photos, !cachedState.center);
  }, [geoJson, photos]);

  function openPreview(photo: MapPhoto) {
    setPreviewPhotoId(photo.photo_id);
    setPreviewMode("image");
  }

  return (
    <section className="map-page">
      <div className="map-shell">
        <div className="map-canvas" ref={containerRef} />
        {loading && <div className="map-message">Loading map photos</div>}
        {error && <div className="map-message error">{error}</div>}
        {!loading && !error && !photos.length && (
          <div className="map-message">No photos with GPS coordinates</div>
        )}
        {selectedPhoto && (
          <button className="map-photo-card" type="button" onClick={() => openPreview(selectedPhoto)}>
            <img src={photoThumbnailUrl(selectedPhoto)} alt={selectedPhoto.filename} loading="lazy" decoding="async" />
            <span>
              <strong>{selectedPhoto.binomial_name ?? selectedPhoto.filename}</strong>
              <small>{selectedPhoto.location ?? selectedPhoto.captured_at ?? selectedPhoto.filename}</small>
            </span>
          </button>
        )}
        {previewPhoto && (
          <MapPreviewPanel
            photo={previewPhoto}
            mode={previewMode}
            onClose={() => setPreviewPhotoId(null)}
            onModeChange={setPreviewMode}
          />
        )}
      </div>
    </section>
  );
}
