// Chunk 688 — dynamic spread against a defaulted callee (spread
// longtail #4, as11): apply_default_args skips spread-carrying
// calls; apply_spread_args expands defaulted slots as
// `j < src.length ? src[j] : default` ternaries. The guard covers
// only the required prefix — a source shorter than the defaulted
// tail falls back per slot.
function d(a: number, b: number = 5): number {
  return a + b;
}
function e(a: number, b: number = 2, c: number = 3): number {
  return a + b * 10 + c * 100;
}
function s(a: string, b: string = "!"): string {
  return a + b;
}
const arr: number[] = [40, 2];
const arr1: number[] = [40];
const one: number[] = [1];
const empty: number[] = [];
const xs1: number[] = [1];
const xs2: number[] = [1, 5];
const xs3: number[] = [1, 5, 7];
const ss: string[] = ["hi"];
// both provided — defaults unused
console.log(d(...arr));
// short source — default kicks in
console.log(d(...arr1));
// prefix covers the required param; spread covers a defaulted slot
console.log(d(7, ...one));
// prefix covers the required param; empty spread — all defaults
console.log(d(7, ...empty));
// multi-default tail, varying coverage
console.log(e(...xs1));
console.log(e(...xs2));
console.log(e(...xs3));
// string lane
console.log(s(...ss));
// static-call regression — padding still applies without spread
console.log(d(1));
console.log(d(1, 2));
// closure-value callee with a default (let-alias walk)
const f = (a: number, b: number = 3): number => a + b;
console.log(f(...arr1));
