export function joinPath(base: string, child: string): string {
  return base ? `${base}/${child}` : child;
}

export function breadcrumb(path: string) {
  const parts = path.split("/").filter(Boolean);
  return parts.map((part, index) => ({
    label: part,
    path: parts.slice(0, index + 1).join("/")
  }));
}
