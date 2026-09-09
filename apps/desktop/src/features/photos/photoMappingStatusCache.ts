import type { PhotoTaxonStatus } from "../../api/mapping";

type StatusLoader = (photoId: number) => Promise<PhotoTaxonStatus>;

export class PhotoMappingStatusCache {
  private readonly values = new Map<number, PhotoTaxonStatus>();
  private readonly pending = new Map<number, Promise<PhotoTaxonStatus>>();
  private readonly loader: StatusLoader;
  private generation = 0;

  constructor(loader: StatusLoader) {
    this.loader = loader;
  }

  load(photoId: number): Promise<PhotoTaxonStatus> {
    if (this.values.has(photoId)) return Promise.resolve(this.values.get(photoId)!);
    const pending = this.pending.get(photoId);
    if (pending) return pending;
    const generation = this.generation;
    const request = this.loader(photoId).then((status) => {
      if (generation === this.generation) this.values.set(photoId, status);
      return status;
    }).finally(() => {
      if (this.pending.get(photoId) === request) this.pending.delete(photoId);
    });
    this.pending.set(photoId, request);
    return request;
  }

  invalidate(photoId: number | null = null): void {
    this.generation += 1;
    if (photoId === null) {
      this.values.clear();
      this.pending.clear();
    } else {
      this.values.delete(photoId);
      this.pending.delete(photoId);
    }
  }
}

export function loadSelectedPhotoMappingStatus(
  photoId: number | null,
  cache: PhotoMappingStatusCache,
): Promise<PhotoTaxonStatus | null> {
  return photoId === null ? Promise.resolve(null) : cache.load(photoId);
}
