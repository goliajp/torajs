const v = { tag: "v" };
type Point = { x: number };
function double(n: number): number {
  return n * 2;
}
export { v as a, v as b, v };
export type { Point, Point as P2 };
export { double, double as twice };
