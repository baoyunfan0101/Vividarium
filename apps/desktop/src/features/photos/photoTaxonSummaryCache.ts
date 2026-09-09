import type { TaxonDisplaySummary } from "../../api/taxonomy";

type SummaryLoader = (photoId: number) => Promise<TaxonDisplaySummary | null>;

export class PhotoTaxonSummaryCache {
  private readonly values = new Map<number, TaxonDisplaySummary | null>();
  private readonly pending = new Map<number, Promise<TaxonDisplaySummary | null>>();
  private readonly loader: SummaryLoader;
  private generation = 0;

  constructor(loader: SummaryLoader) {
    this.loader = loader;
  }

  load(photoId: number): Promise<TaxonDisplaySummary | null> {
    if (this.values.has(photoId)) return Promise.resolve(this.values.get(photoId) ?? null);
    const pending = this.pending.get(photoId);
    if (pending) return pending;
    const generation = this.generation;
    const request = this.loader(photoId).then((summary) => {
      if (generation === this.generation) this.values.set(photoId, summary);
      return summary;
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

export function loadSelectedPhotoTaxonSummary(
  photoId: number | null,
  cache: PhotoTaxonSummaryCache,
): Promise<TaxonDisplaySummary | null> {
  return photoId === null ? Promise.resolve(null) : cache.load(photoId);
}
