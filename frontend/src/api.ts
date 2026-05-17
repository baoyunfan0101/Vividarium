const API_BASE = import.meta.env.VITE_API_BASE ?? "/api";

export type Photo = {
  photo_id: number;
  root: string;
  relative_path: string;
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
  thumbnail_path: string | null;
  status: string;
};

export type PhotoRootMetadata = {
  root: string;
  last_synced_at: string | null;
  sort_order: number;
};

export type TaxaMetadata = {
  knowledge_base_path: string | null;
  knowledge_base_size: number | null;
  knowledge_base_modified_at: string | null;
  last_synced_at: string | null;
};

export type MappingMetadata = {
  last_synced_at: string | null;
  photos_last_synced_at: string | null;
  taxa_last_synced_at: string | null;
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

export type ConfirmationResponse = {
  needs_confirmation: true;
  reason: string;
  message: string;
};

export function photoFileUrl(photoId: number): string {
  return `${API_BASE}/photos/file/${photoId}`;
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
