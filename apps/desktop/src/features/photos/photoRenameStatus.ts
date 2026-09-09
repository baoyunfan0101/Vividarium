export function renameFromTaxonomyStatus(before: string, after: string): string {
  return before === after
    ? "Filename already matches the naming settings."
    : `Renamed to ${after}`;
}
