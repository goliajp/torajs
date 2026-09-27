import type { Pt as Point } from "./lib.ts";

export function norm1(p: Point): number {
  return Math.abs(p.x) + Math.abs(p.y);
}
