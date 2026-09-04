import assert from "node:assert/strict";
import test from "node:test";
import type { Photo } from "../src/api/photos.ts";
import { createPhotoActivationController } from "../src/features/photos/photoActivation.ts";

const photo = { photo_id: 42 } as Photo;

function activationFixture() {
  const selected: number[] = [];
  const images: number[] = [];
  const fullscreen: number[] = [];
  const timers = new Map<number, () => void>();
  let nextTimer = 1;
  const activation = createPhotoActivationController({
    onSelect: (item) => selected.push(item.photo_id),
    onOpenImage: (item) => images.push(item.photo_id),
    onOpenFullscreen: (item) => fullscreen.push(item.photo_id),
  }, {
    setTimeout: (callback) => {
      const timer = nextTimer;
      nextTimer += 1;
      timers.set(timer, callback);
      return timer;
    },
    clearTimeout: (timer) => {
      timers.delete(timer);
    },
  });
  const runTimers = () => {
    const pending = [...timers.values()];
    timers.clear();
    pending.forEach((callback) => callback());
  };
  return { activation, selected, images, fullscreen, timers, runTimers };
}

test("single click selects immediately and opens the image after the delay", () => {
  const fixture = activationFixture();
  fixture.activation.clickPhoto(photo);
  assert.deepEqual(fixture.selected, [42]);
  assert.deepEqual(fixture.images, []);
  assert.equal(fixture.timers.size, 1);
  fixture.runTimers();
  assert.deepEqual(fixture.images, [42]);
});

test("normal double click cancels image opening and opens fullscreen", () => {
  const fixture = activationFixture();
  fixture.activation.clickPhoto(photo);
  fixture.activation.doubleClickPhoto(photo);
  fixture.runTimers();
  assert.deepEqual(fixture.images, []);
  assert.deepEqual(fixture.fullscreen, [42]);
  assert.deepEqual(fixture.selected, [42, 42]);
});

test("selection cancellation clears a pending click without navigation", () => {
  const fixture = activationFixture();
  fixture.activation.clickPhoto(photo);
  fixture.activation.cancelPendingClick();
  fixture.runTimers();
  assert.deepEqual(fixture.images, []);
  assert.deepEqual(fixture.fullscreen, []);
  assert.equal(fixture.timers.size, 0);
});

test("repeated cancellation without an active timer is safe", () => {
  const fixture = activationFixture();
  fixture.activation.cancelPendingClick();
  fixture.activation.cancelPendingClick();
  assert.equal(fixture.timers.size, 0);
});

test("disposing activation clears the pending timer", () => {
  const fixture = activationFixture();
  fixture.activation.clickPhoto(photo);
  fixture.activation.dispose();
  fixture.runTimers();
  assert.deepEqual(fixture.images, []);
  assert.equal(fixture.timers.size, 0);
});
