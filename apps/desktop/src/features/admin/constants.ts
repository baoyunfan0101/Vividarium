import type { ExportModule } from "./types";

export const EXPORT_TABLES: Record<ExportModule, string[]> = {
  photos: ["photos", "photos_dir", "photos_metadata"],
  taxa: ["taxa", "taxa_metadata"],
  mapping: [
    "photos_taxa_mapping",
    "photos_taxa_mapping_metadata",
    "photos_taxa_mapping_taxa"
  ]
};
