import type { StyleSpecification } from "maplibre-gl";
import { readStorage, writeStorage } from "../../lib/storage";

export const MAP_SETTINGS_KEY = "phytoindex.map.settings";
const LEGACY_MAP_PROVIDER_KEY = "phytoindex.map.provider";

export type MapProviderId = "osm" | "tianditu";

export type MapSettings = {
  providerId: MapProviderId;
  tiandituToken: string;
};

export type MapTileProvider = {
  id: MapProviderId;
  name: string;
  kind: "raster" | "vector";
};

export const MAP_TILE_PROVIDERS: MapTileProvider[] = [
  { id: "osm", name: "OpenStreetMap", kind: "raster" },
  { id: "tianditu", name: "Tianditu", kind: "raster" },
];

export function readMapSettings(): MapSettings {
  const legacyProviderId = readStorage<MapProviderId | null>(LEGACY_MAP_PROVIDER_KEY, null);
  const configuredProviderId = import.meta.env.VITE_MAP_PROVIDER;
  const defaults: MapSettings = {
    providerId: isMapProviderId(configuredProviderId) ? configuredProviderId : legacyProviderId ?? "osm",
    tiandituToken: import.meta.env.VITE_TIANDITU_TOKEN?.trim() ?? "",
  };
  const settings = readStorage<Partial<MapSettings>>(MAP_SETTINGS_KEY, defaults);
  return {
    providerId: isMapProviderId(settings.providerId) ? settings.providerId : defaults.providerId,
    tiandituToken: settings.tiandituToken?.trim() || defaults.tiandituToken,
  };
}

export function writeMapSettings(settings: MapSettings): void {
  writeStorage(MAP_SETTINGS_KEY, {
    providerId: settings.providerId,
    tiandituToken: settings.tiandituToken.trim(),
  });
}

export function getMapTileProvider(providerId: MapProviderId, tiandituToken: string): MapTileProvider & { style: StyleSpecification } {
  const provider = MAP_TILE_PROVIDERS.find((item) => item.id === providerId) ?? MAP_TILE_PROVIDERS[0];
  return {
    ...provider,
    style: provider.id === "tianditu" ? tiandituStyle(tiandituToken) : osmStyle(),
  };
}

export function getMapProviderConfigurationError(settings: MapSettings): string | null {
  if (settings.providerId === "tianditu" && !settings.tiandituToken.trim()) {
    return "Tianditu requires an application token (tk). Configure it in Admin > Map.";
  }
  return null;
}

function isMapProviderId(value: unknown): value is MapProviderId {
  return MAP_TILE_PROVIDERS.some((provider) => provider.id === value);
}

function osmStyle(): StyleSpecification {
  return {
    version: 8,
    sources: {
      osm: {
        type: "raster",
        tiles: ["https://tile.openstreetmap.org/{z}/{x}/{y}.png"],
        tileSize: 256,
        attribution: "OpenStreetMap contributors",
      },
    },
    layers: [{ id: "osm", type: "raster", source: "osm" }],
  };
}

function tiandituStyle(token: string): StyleSpecification {
  return {
    version: 8,
    sources: {
      "tianditu-base": {
        type: "raster",
        tiles: tiandituTileUrls("vec", token),
        tileSize: 256,
        maxzoom: 18,
        attribution: "Tianditu",
      },
      "tianditu-labels": {
        type: "raster",
        tiles: tiandituTileUrls("cva", token),
        tileSize: 256,
        maxzoom: 18,
      },
    },
    layers: [
      { id: "tianditu-base", type: "raster", source: "tianditu-base" },
      { id: "tianditu-labels", type: "raster", source: "tianditu-labels" },
    ],
  };
}

function tiandituTileUrls(layer: "vec" | "cva", token: string): string[] {
  const query = [
    "SERVICE=WMTS",
    "REQUEST=GetTile",
    "VERSION=1.0.0",
    `LAYER=${layer}`,
    "STYLE=default",
    "TILEMATRIXSET=w",
    "FORMAT=tiles",
    "TILEMATRIX={z}",
    "TILEROW={y}",
    "TILECOL={x}",
    `tk=${encodeURIComponent(token.trim())}`,
  ].join("&");
  return Array.from({ length: 8 }, (_, index) => `https://t${index}.tianditu.gov.cn/${layer}_w/wmts?${query}`);
}
