import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";

export type Photo = {
  photo_id: number;
  root: string;
  relative_path: string;
  parent_dir: string | null;
  path_depth: number | null;
  filename: string;
  binomial_name: string | null;
  captured_at: string | null;
  location: string | null;
  camera: string | null;
  width: number | null;
  height: number | null;
  file_size: number | null;
  modified_at: number | null;
  longitude: number | null;
  latitude: number | null;
  exif_json: string | null;
  thumbnail_path: string | null;
  status: string;
};

export type MapPhoto = Photo;

export type PhotoRootMetadata = {
  root: string;
  last_synced_at: string | null;
  sort_order: number;
  photo_count: number;
};

export type TaxaMetadata = {
  knowledge_base_path: string | null;
  knowledge_base_size: number | null;
  knowledge_base_modified_at: string | null;
  last_synced_at: string | null;
  taxa_count: number;
};

export type MappingMetadata = {
  last_synced_at: string | null;
  photos_last_synced_at: string | null;
  taxa_last_synced_at: string | null;
  mapped_photo_count: number;
  mapping_taxa_count: number;
};

export type DirectoryListing = {
  root: string;
  relative_dir: string;
  directories: string[];
  files: Photo[];
};

export type DirectoryListingPage = DirectoryListing & {
  next_cursor: string | null;
  directory_count: number;
  file_count: number;
};

export type Taxon = {
  taxon_id: number;
  rank: string;
  name: string;
  parent_id: number | null;
  binomial_name: string | null;
};

export type MappingNode = {
  taxon: Taxon | null;
  photo_ids: number[];
  children: Taxon[];
};

export type TaxonSuggestion = Taxon;

export type ConfirmationResponse = {
  needs_confirmation: true;
  reason: string;
  message: string;
};

export type OperationState = {
  module: "photos" | "taxa" | "mapping";
  task_id: string | null;
  operation: string | null;
  running: boolean;
  started_at: string | null;
  finished_at: string | null;
  message: string;
  processed: number;
  total: number | null;
  result: unknown;
  error: string | null;
};

export type OperationsStatus = Record<OperationState["module"], OperationState>;

// Photos

export function photoFileUrl(photo: Photo): string {
  return `${convertFileSrc(`photo/${photo.photo_id}`, "phytoindex")}?${photoVersionParams(photo)}`;
}

export function photoThumbnailUrl(photo: Photo): string {
  return `${convertFileSrc(`thumbnail/${photo.photo_id}`, "phytoindex")}?${photoVersionParams(photo)}`;
}

function photoVersionParams(photo: Photo): string {
  const version = [
    photo.root,
    photo.relative_path,
    photo.modified_at ?? "",
    photo.file_size ?? "",
  ].join("|");
  return new URLSearchParams({ v: version }).toString();
}

export async function getRoots(): Promise<string[]> {
  return (await getPhotoRootsMetadata()).map((item) => item.root);
}

export function getPhotoRootsMetadata(): Promise<PhotoRootMetadata[]> {
  return invoke("get_photo_roots_metadata");
}

export function savePhotoRoots(roots: string[]): Promise<PhotoRootMetadata[]> {
  return invoke("save_photo_roots", { roots });
}

// Taxa

export function getTaxaMetadata(): Promise<TaxaMetadata> {
  return invoke("get_taxa_metadata");
}

export function saveKnowledgeBasePath(knowledgeBasePath: string | null): Promise<TaxaMetadata> {
  return invoke("save_knowledge_base_path", { knowledgeBasePath });
}

// Photos-taxa mapping metadata

export function getMappingMetadata(): Promise<MappingMetadata> {
  return invoke("get_mapping_metadata");
}

// Operation status

export function getOperationsStatus(): Promise<OperationsStatus> {
  return invoke("get_operations_status");
}

// Local path pickers

export async function selectLocalDirectory(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false });
  return typeof selected === "string" ? selected : null;
}

export async function selectLocalFile(): Promise<string | null> {
  const selected = await open({
    directory: false,
    multiple: false,
    filters: [{ name: "Excel workbook", extensions: ["xlsx", "xlsm", "xls"] }]
  });
  return typeof selected === "string" ? selected : null;
}

// Photos browsing and lookup

export async function browsePhotos(root: string, relativeDir = ""): Promise<DirectoryListing> {
  return browsePhotosPage(root, relativeDir, null, 500);
}

export function browsePhotosPage(
  root: string,
  relativeDir = "",
  cursor: string | null = null,
  limit = 160,
): Promise<DirectoryListingPage> {
  return invoke("browse_photos_page", { root, relativeDir, cursor, limit });
}

export function getAllPhotos(): Promise<Photo[]> {
  return invoke("get_all_photos");
}

export function getChangedPhotos(): Promise<Photo[]> {
  return invoke("get_changed_photos");
}

export function getPhoto(photoId: number): Promise<Photo> {
  return invoke("get_photo", { photoId });
}

// Map

export function getMapPhotos(): Promise<MapPhoto[]> {
  return invoke("get_map_photos");
}

// Photos-taxa mapping

export function getMappingRoot(): Promise<MappingNode> {
  return invoke("get_mapping_root");
}

export function getMappingTaxon(taxonId: number): Promise<MappingNode> {
  return invoke("get_mapping_taxon", { taxonId });
}

export function searchMappingByName(name: string): Promise<MappingNode> {
  return invoke("search_mapping_by_name", { name });
}

export function searchMappingByBinomial(binomialName: string): Promise<MappingNode> {
  return invoke("search_mapping_by_binomial", { binomialName });
}

export function suggestMappingTaxa(query: string, mode: "name" | "binomial"): Promise<TaxonSuggestion[]> {
  return invoke("suggest_mapping_taxa", { query, mode });
}

// Operations and exports

export function startPhotosUpdate(roots: string[]): Promise<unknown> {
  return invoke("start_photos_update", { roots });
}

export function startPhotosRebuild(roots: string[]): Promise<unknown> {
  return invokeWithConfirmation("start_photos_rebuild", { roots });
}

export function startTaxaUpdate(knowledgeBasePath: string | null): Promise<unknown> {
  return invokeWithConfirmation("start_taxa_update", { knowledgeBasePath });
}

export function startTaxaRebuild(knowledgeBasePath: string | null): Promise<unknown> {
  return invokeWithConfirmation("start_taxa_rebuild", { knowledgeBasePath });
}

export function startMappingUpdate(): Promise<unknown> {
  return invokeWithConfirmation("start_mapping_update", {});
}

export function startMappingRebuild(): Promise<unknown> {
  return invokeWithConfirmation("start_mapping_rebuild", {});
}

async function invokeWithConfirmation(command: string, args: Record<string, unknown>): Promise<unknown> {
  const result = await invoke<unknown>(command, { ...args, force: false });
  if (isConfirmation(result)) {
    const confirmed = window.confirm(result.message);
    if (!confirmed) {
      return result;
    }
    return invoke(command, { ...args, force: true });
  }
  return result;
}

export async function waitForOperation(operation: OperationState): Promise<unknown> {
  if (!operation.task_id) {
    return { result: operation.result };
  }

  let latest: OperationState | null = null;
  let wake: (() => void) | null = null;
  const unlisten = await listen<OperationState>("operation-progress", (event) => {
    if (event.payload.task_id !== operation.task_id) {
      return;
    }
    latest = event.payload;
    wake?.();
  });
  try {
    while (true) {
      const current = latest ?? (await getOperationsStatus())[operation.module];
      if (current.task_id !== operation.task_id) {
        throw new Error(`${operation.module} operation was replaced before it finished`);
      }
      if (!current.running) {
        if (current.error) {
          throw new Error(current.error);
        }
        return { result: current.result };
      }
      await new Promise<void>((resolve) => {
        const timer = window.setTimeout(resolve, 1000);
        wake = () => {
          window.clearTimeout(timer);
          resolve();
        };
      });
      wake = null;
    }
  } finally {
    unlisten();
  }
}

export async function downloadTable(tableName: string): Promise<string | null> {
  const outputPath = await save({
    defaultPath: `${tableName}.csv`,
    filters: [{ name: "CSV", extensions: ["csv"] }]
  });
  if (!outputPath) {
    return null;
  }
  await invoke("export_table", { tableName, outputPath });
  return outputPath.split(/[\\/]/).pop() ?? `${tableName}.csv`;
}

function isConfirmation(value: unknown): value is ConfirmationResponse {
  return Boolean(
    value &&
      typeof value === "object" &&
      "needs_confirmation" in value &&
      (value as ConfirmationResponse).needs_confirmation
  );
}

export function operationFromResponse(value: unknown): OperationState | null {
  if (
    value &&
    typeof value === "object" &&
    "operation" in value &&
    (value as { operation?: unknown }).operation &&
    typeof (value as { operation?: unknown }).operation === "object"
  ) {
    return (value as { operation: OperationState }).operation;
  }
  return null;
}
