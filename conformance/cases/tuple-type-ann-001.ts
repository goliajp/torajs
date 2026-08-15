const pos = [3, 4] as [number, number];
console.log(pos[0] + pos[1]);

const mixed = [1, "two"] as [number, string];
console.log(mixed[1], typeof mixed[0]);

let named: [number, number, number] = [7, 8, 9];
console.log(named[2], named.length);

function dist(p: [number, number]): number {
  return p[0] * p[0] + p[1] * p[1];
}
console.log(dist([3, 4]));

const nested = [
  [1, 2],
  [3, 4],
] as [number, number][];
console.log(nested[1][0]);

const nul: [number, number] | null = null;
console.log(nul === null);

type Vec2 = [number, number];
const v: Vec2 = [5, 6];
console.log(v[0] * v[1]);
