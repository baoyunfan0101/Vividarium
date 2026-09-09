import { call } from "./client";
import type { Page } from "./common";
import { demoPhotos, type Photo } from "./photos";
import {
  demoTaxa,
  demoTaxonDisplaySummary,
  demoTaxonSummary,
  type TaxonDisplaySummary,
  type TaxonDisplayNames,
  type TaxonomyNameType,
  type TaxonRank,
  type TaxonSummary,
} from "./taxonomy";
import { demoOperation, type OperationState } from "./tasks";

export type PhotoTaxonStatus = "matched" | "ambiguous" | "unmatched" | "processing";
export type PhotoMappingSummary = { photo_id: number; taxon_id: number | null; status: PhotoTaxonStatus };
export type PhotoMappingListItem = { photo: Photo; mapping: PhotoMappingSummary };
export type PhotoMatchedName = { name_id: number; name_type: TaxonomyNameType; name: string };
export type PhotoTaxonCandidate = {
  summary: TaxonSummary;
  matched_names: PhotoMatchedName[];
  accepted_names: TaxonDisplayNames;
};
export type PhotoMappingDetail = {
  mapping: PhotoMappingSummary;
  matched_names: PhotoMatchedName[];
  candidates: PhotoTaxonCandidate[];
};
export type MappingMetadata = {
  mapped_photo_count: number;
  unmatched_photo_count: number;
  ambiguous_photo_count: number;
  processing_photo_count: number;
  mapping_taxa_count: number;
};
export type PhotoTaxonUsage = {
  taxon_id: number;
  rank: TaxonRank;
  names: TaxonDisplayNames;
  direct_photo_count: number;
  subtree_photo_count: number;
};
export type PhotoTaxonNode = { taxon: PhotoTaxonUsage | null; subtree_photo_count: number };
export type PhotoTaxonEntryCounts = { taxon_count: number; photo_count: number };
export type PhotoTaxonItem =
  | { kind: "taxon"; taxon: PhotoTaxonUsage }
  | { kind: "photo"; photo: Photo };

const demoMappings = new Map<number, PhotoMappingSummary>(demoPhotos.map((photo, index) => [
  photo.photo_id,
  {
    photo_id: photo.photo_id,
    taxon_id: index % 5 === 3 ? null : demoTaxa[index % demoTaxa.length].taxon_id,
    status: index % 11 === 0 ? "processing" : index % 7 === 0 ? "ambiguous" : index % 5 === 3 ? "unmatched" : "matched",
  },
]));

export const startPhotoMapping = () =>
  call<{ operation: OperationState }>("start_photo_mapping", undefined, () => ({
    operation: demoOperation("mapping", "Mapping complete"),
  }));
export const getPhotoMapping = (photoId: number) =>
  call<PhotoMappingSummary>("get_photo_mapping", { photoId }, () =>
    demoMappings.get(photoId) ?? { photo_id: photoId, taxon_id: null, status: "unmatched" });
export const getPhotoTaxonDisplaySummary = (photoId: number) =>
  call<TaxonDisplaySummary | null>("get_photo_taxon_display_summary", { photoId }, () => {
    const mapping = demoMappings.get(photoId);
    return mapping?.status === "matched" && mapping.taxon_id !== null
      ? demoTaxonDisplaySummary(mapping.taxon_id)
      : null;
  });
export const getPhotoMappingDetail = (photoId: number) =>
  call<PhotoMappingDetail>("get_photo_mapping_detail", { photoId }, () => {
    const mapping = demoMappings.get(photoId);
    const candidates = mapping?.status === "ambiguous"
      ? demoTaxa.slice(0, 3).map((taxon) => ({
          summary: demoTaxonSummary(taxon),
          matched_names: taxon.matches,
          accepted_names: taxon.names,
        }))
      : [];
    return {
      mapping: mapping ?? { photo_id: photoId, taxon_id: null, status: "unmatched" },
      matched_names: [],
      candidates,
    };
  });
export const clearPhotoMapping = (photoId: number) =>
  call<PhotoMappingSummary>("clear_photo_mapping", { photoId }, () => {
    const mapping: PhotoMappingSummary = { photo_id: photoId, taxon_id: null, status: "unmatched" };
    demoMappings.set(photoId, mapping);
    return mapping;
  });
export const setPhotoMapping = (photoId: number, taxonId: number) =>
  call<PhotoMappingSummary>("set_photo_mapping", { photoId, taxonId }, () => {
    const mapping: PhotoMappingSummary = { photo_id: photoId, taxon_id: taxonId, status: "matched" };
    demoMappings.set(photoId, mapping);
    return mapping;
  });
export const remapPhoto = (photoId: number) =>
  call<PhotoMappingSummary>("remap_photo", { photoId }, () => getPhotoMapping(photoId));
export const getMappingMetadata = () =>
  call<MappingMetadata>("get_mapping_metadata", undefined, () => ({
    mapped_photo_count: 68,
    unmatched_photo_count: 14,
    ambiguous_photo_count: 8,
    processing_photo_count: 6,
    mapping_taxa_count: 21,
  }));
export const listPhotosByMappingStatus = (status: PhotoTaxonStatus, cursor: string | null = null, limit = 80) =>
  call<Page<PhotoMappingListItem>>("list_photos_by_mapping_status", { status, cursor, limit }, () => ({
    items: demoPhotos
      .map((photo) => ({ photo, mapping: demoMappings.get(photo.photo_id)! }))
      .filter((item) => item.mapping.status === status)
      .slice(0, limit),
    next_cursor: null,
  }));
export const searchPhotosByMappingStatus = (
  status: PhotoTaxonStatus,
  query: string,
  cursor: string | null = null,
  limit = 80,
) => call<Page<PhotoMappingListItem>>("search_photos_by_mapping_status", { status, query, cursor, limit }, () => ({
  items: demoPhotos
    .map((photo) => ({ photo, mapping: demoMappings.get(photo.photo_id)! }))
    .filter((item) => item.mapping.status === status && item.photo.filename.toLowerCase().includes(query.toLowerCase()))
    .slice(0, limit),
  next_cursor: null,
}));
export const getPhotoTaxonNode = (taxonId: number | null, showEmpty = false) =>
  call<PhotoTaxonNode>("get_photo_taxon_node", { taxonId, showEmpty }, () => ({
    taxon: taxonId
      ? { taxon_id: taxonId, rank: "species", names: demoTaxa[0].names, direct_photo_count: 12, subtree_photo_count: 12 }
      : null,
    subtree_photo_count: demoPhotos.length,
  }));
export const getPhotoTaxonCounts = (taxonId: number | null) =>
  call<PhotoTaxonEntryCounts>("get_photo_taxon_counts", { taxonId }, () => ({
    taxon_count: taxonId === null ? demoTaxa.length : 0,
    photo_count: taxonId === null ? 0 : demoPhotos.filter((photo) => demoMappings.get(photo.photo_id)?.taxon_id === taxonId).length,
  }));
export const browsePhotoTaxon = (
  taxonId: number | null,
  showEmpty = false,
  cursor: string | null = null,
  limit = 80,
) => call<Page<PhotoTaxonItem>>("browse_photo_taxon", { taxonId, showEmpty, cursor, limit }, () => ({
  items: taxonId === null
    ? demoTaxa.map((taxon) => ({
        kind: "taxon" as const,
        taxon: {
          taxon_id: taxon.taxon_id,
          rank: taxon.rank,
          names: taxon.names,
          direct_photo_count: 12,
          subtree_photo_count: 24,
        },
      }))
    : demoPhotos
        .filter((photo) => demoMappings.get(photo.photo_id)?.taxon_id === taxonId)
        .slice(0, limit)
        .map((photo) => ({ kind: "photo" as const, photo })),
  next_cursor: null,
}));
