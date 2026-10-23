/** Translate a closed world-Y interval without shrinking it at either bound. */
export function shiftLayerRange(range: readonly [number, number], bounds: readonly [number, number], delta: number): [number, number] {
  const movement = Math.max(bounds[0] - range[0], Math.min(bounds[1] - range[1], delta));
  return [range[0] + movement, range[1] + movement];
}
