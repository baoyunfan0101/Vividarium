import type { PhotoRootMetadata } from "../../bridge";

export type ExportModule = "photos" | "taxa" | "mapping";
export type RootRow = PhotoRootMetadata & {
  selected: boolean;
};
