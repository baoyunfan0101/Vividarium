import type { MapPhoto } from "../../api";

export type MapPhotoProperties = {
  photo_id: number;
  filename: string;
  binomial_name: string | null;
  captured_at: string | null;
  location: string | null;
};

export type MapPhotoFeatureCollection = {
  type: "FeatureCollection";
  features: Array<{
    type: "Feature";
    geometry: {
      type: "Point";
      coordinates: [number, number];
    };
    properties: MapPhotoProperties;
  }>;
};

export function mapPhotosToGeoJson(photos: MapPhoto[]): MapPhotoFeatureCollection {
  return {
    type: "FeatureCollection",
    features: photos.map((photo) => ({
      type: "Feature",
      geometry: {
        type: "Point",
        coordinates: [photo.longitude ?? 0, photo.latitude ?? 0],
      },
      properties: {
        photo_id: photo.photo_id,
        filename: photo.filename,
        binomial_name: photo.binomial_name,
        captured_at: photo.captured_at,
        location: photo.location,
      },
    })),
  };
}

export function centerForPhotos(photos: MapPhoto[]): [number, number] {
  if (!photos.length) {
    return [105, 35];
  }
  const sum = photos.reduce(
    (current, photo) => ({
      longitude: current.longitude + (photo.longitude ?? 0),
      latitude: current.latitude + (photo.latitude ?? 0),
    }),
    { longitude: 0, latitude: 0 },
  );
  return [sum.longitude / photos.length, sum.latitude / photos.length];
}
