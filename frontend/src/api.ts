const API_BASE = import.meta.env.VITE_API_BASE ?? "/api";

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

export function photoFileUrl(photo: Photo): string {
  return `${API_BASE}/photos/file/${photo.photo_id}?${photoVersionParams(photo)}`;
}

export function photoThumbnailUrl(photo: Photo): string {
  return `${API_BASE}/photos/thumbnail/${photo.photo_id}?${photoVersionParams(photo)}`;
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
  const data = await request<{ roots: string[] }>("/photos/roots");
  return data.roots;
}

export async function getPhotoRootsMetadata(): Promise<PhotoRootMetadata[]> {
  const data = await request<{ metadata: PhotoRootMetadata[] }>("/photos/roots");
  return data.metadata;
}

export async function savePhotoRoots(roots: string[]): Promise<PhotoRootMetadata[]> {
  const data = await request<{ metadata: PhotoRootMetadata[] }>("/photos/roots", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ roots })
  });
  return data.metadata;
}

export async function getTaxaMetadata(): Promise<TaxaMetadata> {
  const data = await request<{ metadata: TaxaMetadata }>("/taxa/latest-update");
  return data.metadata;
}

export async function saveKnowledgeBasePath(knowledgeBasePath: string | null): Promise<TaxaMetadata> {
  const data = await request<{ metadata: TaxaMetadata }>("/taxa/knowledge-base", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ knowledge_base_path: knowledgeBasePath })
  });
  return data.metadata;
}

export async function getMappingMetadata(): Promise<MappingMetadata> {
  const data = await request<{ metadata: MappingMetadata }>("/mapping/photos-taxa/latest-update");
  return data.metadata;
}

export async function getOperationsStatus(): Promise<OperationsStatus> {
  const data = await request<{ operations: OperationsStatus }>("/operations/status");
  return data.operations;
}

export async function selectLocalDirectory(): Promise<string | null> {
  const data = await request<{ path: string | null }>("/local/select-directory");
  return data.path;
}

export async function selectLocalFile(): Promise<string | null> {
  const data = await request<{ path: string | null }>("/local/select-file");
  return data.path;
}

export async function browsePhotos(root: string, relativeDir = ""): Promise<DirectoryListing> {
  const params = new URLSearchParams({ root, relative_dir: relativeDir });
  return request<DirectoryListing>(`/photos/browse?${params.toString()}`);
}

export async function getAllPhotos(): Promise<Photo[]> {
  const data = await request<{ photos: Photo[] }>("/photos/all");
  return data.photos;
}

export async function getChangedPhotos(): Promise<Photo[]> {
  const data = await request<{ photos: Photo[] }>("/photos/changed");
  return data.photos;
}

export async function getPhoto(photoId: number): Promise<Photo> {
  const data = await request<{ photo: Photo }>(`/photos/${photoId}`);
  return data.photo;
}

export async function getMappingRoot(): Promise<MappingNode> {
  return request<MappingNode>("/mapping/photos-taxa/root");
}

export async function getMappingTaxon(taxonId: number): Promise<MappingNode> {
  return request<MappingNode>(`/mapping/photos-taxa/taxon/${taxonId}`);
}

export async function searchMappingByName(name: string): Promise<MappingNode> {
  const params = new URLSearchParams({ name });
  return request<MappingNode>(`/mapping/photos-taxa/search?${params.toString()}`);
}

export async function searchMappingByBinomial(binomialName: string): Promise<MappingNode> {
  const params = new URLSearchParams({ binomial_name: binomialName });
  return request<MappingNode>(`/mapping/photos-taxa/search-binomial?${params.toString()}`);
}

export async function suggestMappingTaxa(query: string, mode: "name" | "binomial"): Promise<TaxonSuggestion[]> {
  const params = new URLSearchParams({ query, mode });
  const data = await request<{ suggestions: TaxonSuggestion[] }>(`/mapping/photos-taxa/suggest?${params.toString()}`);
  return data.suggestions;
}

export async function runMutation(path: string, body: object): Promise<unknown> {
  const result = await request<unknown>(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body)
  });
  if (isConfirmation(result)) {
    const confirmed = window.confirm(result.message);
    if (!confirmed) {
      return result;
    }
    return request(path, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ ...body, force: true })
    });
  }
  return result;
}

export async function waitForOperation(operation: OperationState): Promise<unknown> {
  if (!operation.task_id) {
    return { result: operation.result };
  }

  while (true) {
    await delay(700);
    const operations = await getOperationsStatus();
    const current = operations[operation.module];
    if (current.task_id !== operation.task_id) {
      throw new Error(`${operation.module} operation was replaced before it finished`);
    }
    if (current.running) {
      continue;
    }
    if (current.error) {
      throw new Error(current.error);
    }
    return { result: current.result };
  }
}

export async function downloadTable(path: string, tableName: string): Promise<string> {
  const params = new URLSearchParams({ table_name: tableName });
  const response = await fetch(`${API_BASE}${path}?${params.toString()}`);
  if (!response.ok) {
    const data = await response.json().catch(() => null);
    const detail = data?.detail ?? response.statusText;
    throw new Error(typeof detail === "string" ? detail : JSON.stringify(detail));
  }

  const blob = await response.blob();
  const filename = filenameFromDisposition(response.headers.get("content-disposition")) ?? `${tableName}.csv`;
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
  return filename;
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, init);
  const data = await response.json().catch(() => null);
  if (!response.ok) {
    const detail = data?.detail ?? response.statusText;
    throw new Error(typeof detail === "string" ? detail : JSON.stringify(detail));
  }
  return data as T;
}

function filenameFromDisposition(disposition: string | null): string | null {
  if (!disposition) {
    return null;
  }
  const match = disposition.match(/filename="?([^"]+)"?/i);
  return match?.[1] ?? null;
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

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}
