type SurfaceSelection = Pick<Selection, "getRangeAt" | "isCollapsed" | "rangeCount">;

export function selectionIntersectsElement(
  element: HTMLElement,
  selection: SurfaceSelection | null = window.getSelection(),
): boolean {
  if (!selection || selection.isCollapsed || selection.rangeCount === 0) return false;
  for (let index = 0; index < selection.rangeCount; index += 1) {
    try {
      if (selection.getRangeAt(index).intersectsNode(element)) return true;
    } catch {
      continue;
    }
  }
  return false;
}
