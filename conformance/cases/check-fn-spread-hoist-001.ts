// Chunk 689 — non-Ident spread sources (spread longtail #5, as6):
// a call / member source hoists into a compiler temp ahead of the
// statement (Stmt::Multi shares the surrounding scope), then the
// existing Ident-source walks expand it. Hoist fires only on the
// statement's first-evaluation chain with an effect-free prefix.
function sum2(a: number, b: number): number {
  return a + b;
}
function sum3(a: number, b: number, c: number): number {
  return a + b * 10 + c * 100;
}
function join2(a: string, b: string): string {
  return a + b;
}
function mk(): number[] {
  return [40, 2];
}
function mk2(): number[] {
  return [2, 3];
}
function mk1(): number[] {
  return [37];
}
function mks(): string[] {
  return ["4", "2"];
}
function d(a: number, b: number = 5): number {
  return a + b;
}
// call source inside a nested call (the as6 shape)
console.log(sum2(...mk()));
// effect-free prefix + call source
console.log(sum3(1, ...mk2()));
// Math walk rides the hoisted temp
console.log(Math.max(...mk()));
// push walk rides the hoisted temp
const xs: number[] = [7];
xs.push(...mk());
console.log(xs.length, xs[1], xs[2]);
// defaulted callee + call source (chunk 688 composition)
console.log(d(...mk1()));
// let-init position
const n: number = sum2(...mk());
console.log(n);
// return position
function wrap(): number {
  return sum2(...mk());
}
console.log(wrap());
// member source
const o = { arr: [40, 2] };
console.log(sum2(...o.arr));
// string-lane call source
console.log(join2(...mks()));
