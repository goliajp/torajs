// Non-trailing / multi / non-Ident-source spreads against a bare
// top-FnDecl callee: the shapes the static expanders decline route
// through the runtime spread lane via the forwarder-wrapped callee.
function four(a: number, b: number, c: number, d: number): number {
  return a * 1000 + b * 100 + c * 10 + d;
}
const mixed = [2, 3];
console.log(four(1, ...mixed, 4));
const a2 = [1, 2];
const b2 = [3, 4];
console.log(four(...a2, ...b2));
function tailArr(): number[] {
  return [3, 4];
}
console.log(four(1, 2, ...tailArr()));
function restStarved(a: number, b: number, c: number, ...r: number[]): number {
  return a * 100 + b * 10 + c + r.length;
}
const xs = [1, 2, 3, 4, 5];
console.log(restStarved(...xs));
function noann(a: number, b: number, c: number) {
  return a * 100 + b * 10 + c;
}
const m2 = [2];
console.log(noann(1, ...m2, 3));
