import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

function source(path: string) {
  return readFileSync(new URL(path, import.meta.url), "utf8");
}

test("full image zoom uses the two-dimensional transform path", () => {
  const media = source("../src/features/photos/PhotoMedia.tsx");
  const styles = source("../src/styles/photos.css");
  assert.match(media, /translate\(-50%, -50%\) translate\(/);
  assert.doesNotMatch(media, /translate3d|scale3d/);
  assert.doesNotMatch(styles, /backface-visibility|will-change:\s*transform/);
});

test("image keyboard handling gives fullscreen Escape priority", () => {
  const display = source("../src/features/photos/PhotoDisplay.tsx");
  assert.match(display, /modeRef\.current === "image" && event\.key === "Enter"/);
  assert.match(display, /onEnterFullscreenRef\.current\?\.\(\)/);
  assert.match(display, /if \(isPhotoFullscreenActive\(\)\) return;/);
  assert.match(display, /setMode\("thumbnails"\)/);
});

test("native fullscreen capability and shared presentation are wired", () => {
  const capability = source("../src-tauri/capabilities/desktop.json");
  const shell = source("../src/app/DesktopShell.tsx");
  assert.match(capability, /core:window:allow-set-fullscreen/);
  assert.match(shell, /getCurrentWindow\(\)\.setFullscreen\(true\)/);
  assert.match(shell, /appWindow\.setFullscreen\(false\)/);
  assert.match(shell, /<PhotoFullscreenPresentation/);
  const presentation = shell.slice(shell.indexOf("<PhotoFullscreenPresentation"), shell.indexOf("/>", shell.indexOf("<PhotoFullscreenPresentation")));
  assert.match(presentation, /photo=\{fullscreenPhoto\}\s+onExit=\{exitPhotoFullscreen\}/);
  assert.doesNotMatch(presentation, /handlers=\{handlers\}/);
});

test("fullscreen exit waits for a non-fullscreen resize before unmounting and restoring focus", () => {
  const shell = source("../src/app/DesktopShell.tsx");
  const open = shell.slice(shell.indexOf("const openPhotoFullscreen"), shell.indexOf("const exitPhotoFullscreen"));
  const exit = shell.slice(shell.indexOf("const exitPhotoFullscreen"), shell.indexOf("const handlers"));

  assert.match(open, /fullscreenReturnFocusRef\.current = onReturnFocus \?\? null;/);
  assert.match(open, /setFullscreenPhoto\(photo\);\s*setPhotoFullscreenActive\(true\);\s*window\.requestAnimationFrame\(/);
  assert.match(open, /requestAnimationFrame\(\(\) => \{\s*if \(requestId !== fullscreenRequestRef\.current\) return;\s*void getCurrentWindow\(\)\.setFullscreen\(true\)\.catch/);
  assert.match(open, /setPhotoFullscreenActive\(false\);\s*setFullscreenPhoto\(null\);\s*fullscreenReturnFocusRef\.current = null;/);

  const listener = exit.indexOf("await appWindow.onResized");
  const request = exit.indexOf("await appWindow.setFullscreen(false)");
  assert.ok(listener >= 0 && listener < request);
  assert.match(exit, /appWindow\.isFullscreen\(\)\s*\.then\(\(fullscreen\) => \{\s*if \(fullscreen\) return;\s*restoreAfterExit\(\);/);
  assert.match(exit, /setPhotoFullscreenActive\(false\);\s*setFullscreenPhoto\(null\);\s*\+\+fullscreenRequestRef\.current;\s*window\.requestAnimationFrame/);
  assert.match(exit, /const restore = fullscreenReturnFocusRef\.current;\s*fullscreenReturnFocusRef\.current = null;\s*restore\?\.\(\);/);
  assert.match(exit, /cleanup\(\);\s*if \(requestId !== fullscreenRequestRef\.current\) return;\s*completed = true;\s*reportActiveStatus/);
  assert.match(exit, /catch \(nextError\) \{\s*cleanup\(\);\s*if \(requestId !== fullscreenRequestRef\.current\) return;\s*completed = true;\s*reportActiveStatus/);
});

test("photo-item double click opens fullscreen while full-image double click keeps zoom", () => {
  const display = source("../src/features/photos/PhotoDisplay.tsx");
  const activation = source("../src/features/photos/photoActivation.ts");
  const media = source("../src/features/photos/PhotoMedia.tsx");
  const browser = source("../src/features/photos/PhotoBrowser.tsx");
  const views = source("../src/features/photos/PhotosView.tsx");

  assert.match(display, /onOpenFullscreen: \(photo: Photo\) => void;/);
  assert.match(activation, /callbacks\.onOpenFullscreen\(photo\);/);
  assert.doesNotMatch(display, /onOpenDetails/);
  assert.match(browser, /onOpenFullscreen: openFullscreen/);
  assert.equal((views.match(/onOpenFullscreen: openFullscreen/g) ?? []).length, 2);
  assert.match(media, /onDoubleClick=\{toggleDefaultZoom\}/);
});

test("fullscreen presentation owns focus while it is mounted", () => {
  const presentation = source("../src/features/photos/PhotoFullscreenPresentation.tsx");
  assert.match(presentation, /const presentationRef = useRef<HTMLDivElement>\(null\);/);
  assert.match(presentation, /presentationRef\.current\?\.focus\(\{ preventScroll: true \}\);/);
  assert.match(presentation, /ref=\{presentationRef\} className="photo-fullscreen-presentation" tabIndex=\{-1\}/);
});

test("fullscreen presentation is a pure photo-viewing mode without a context menu", () => {
  const presentation = source("../src/features/photos/PhotoFullscreenPresentation.tsx");
  assert.match(presentation, /<PhotoStage photo=\{photo\} \/>/);
  assert.doesNotMatch(presentation, /usePhotoInteraction|PhotoOpenHandlers|handlers|onContextMenu|contextMenu/);
});
