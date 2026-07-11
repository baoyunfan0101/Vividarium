import type { StyleSpecification } from "maplibre-gl";

export const MAP_PROVIDER_KEY = "phytoindex.map.provider";

export type MapTileProvider = {
  id: string;
  name: string;
  kind: "raster" | "vector";
  style: StyleSpecification;
};

export const MAP_TILE_PROVIDERS: MapTileProvider[] = [
  {
    id: "osm",
    name: "OpenStreetMap",
    kind: "raster",
    style: {
      version: 8,
      sources: {
        osm: {
          type: "raster",
          tiles: ["https://tile.openstreetmap.org/{z}/{x}/{y}.png"],
          tileSize: 256,
          attribution: "© OpenStreetMap contributors",
        },
      },
      layers: [
        {
          id: "osm",
          type: "raster",
          source: "osm",
        },
      ],
    },
  },
];

export function getMapTileProvider(providerId: string): MapTileProvider {
  return MAP_TILE_PROVIDERS.find((provider) => provider.id === providerId) ?? MAP_TILE_PROVIDERS[0];
}
