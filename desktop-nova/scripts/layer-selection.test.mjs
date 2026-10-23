import test from "node:test";
import assert from "node:assert/strict";
import { shiftLayerRange } from "../ui/windows/main/pages/flake/layer3DSelection.ts";
test("negative world-Y intervals move together and stop without shrinking", () => {
  assert.deepEqual(shiftLayerRange([-10, -5], [-12, 2], -20), [-12, -7]);
  assert.deepEqual(shiftLayerRange([-10, -5], [-12, 2], 20), [-3, 2]);
  assert.deepEqual(shiftLayerRange([-3, 2], [-12, 2], 1), [-3, 2]);
});
test("single layers and inclusive interval width remain stable", () => {
  assert.deepEqual(shiftLayerRange([20, 20], [20, 21], 1), [21, 21]);
  assert.deepEqual(shiftLayerRange([-2, 2], [-2, 2], -1), [-2, 2]);
  assert.equal(shiftLayerRange([2, 8], [-64, 319], 1)[1] - shiftLayerRange([2, 8], [-64, 319], 1)[0] + 1, 7);
});
