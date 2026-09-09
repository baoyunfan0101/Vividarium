let active = false;

export function isPhotoFullscreenActive(): boolean {
  return active;
}

export function setPhotoFullscreenActive(next: boolean): void {
  active = next;
}
