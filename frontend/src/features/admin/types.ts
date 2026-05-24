import type { PhotoRootMetadata } from "../../api";

export type ExportModule = "photos" | "taxa" | "mapping";
export type RootRow = PhotoRootMetadata & {
  selected: boolean;
};
