export type FlakeStateHintMode = "mask" | "replace";
export type FlakeOverlayBlendMode = "normal" | "subtract";

export interface FlakeStateHintRule {
  mode: FlakeStateHintMode;
  imageRelPaths?: string[];
  iconBlockIds?: string[];
  baseImageRelPaths?: string[];
  baseImageRotateQuarterTurns?: number;
  overlayIconBlockIds?: string[];
  overlayBlendMode?: FlakeOverlayBlendMode;
}