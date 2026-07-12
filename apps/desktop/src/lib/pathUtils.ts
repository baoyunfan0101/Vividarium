export function joinPath(base: string, child: string): string {
  return base ? `${base}/${child}` : child;
}

export function displayPath(path: string): string {
  const uncPrefix = "\\\\?\\UNC\\";
  if (path.startsWith(uncPrefix)) {
    return `\\\\${path.slice(uncPrefix.length)}`;
  }

  const extendedPrefix = "\\\\?\\";
  return path.startsWith(extendedPrefix) ? path.slice(extendedPrefix.length) : path;
}

export function breadcrumb(path: string) {
  const parts = path.split("/").filter(Boolean);
  return parts.map((part, index) => ({
    label: part,
    path: parts.slice(0, index + 1).join("/")
  }));
}
