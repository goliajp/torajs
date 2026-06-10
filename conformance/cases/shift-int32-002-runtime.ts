// L3a-8 — int32 shift/bitwise semantics through runtime values
// (function params + loop-carried counters), exercising the non-const
// emit path: ToInt32 operand normalization, shift-count masking, and
// `<<` result truncation must match bun.
function shl(a: number, b: number): number {
  return a << b;
}
function shr(a: number, b: number): number {
  return a >> b;
}
function ushr(a: number, b: number): number {
  return a >>> b;
}
function band(a: number, b: number): number {
  return a & b;
}
function bnot(a: number): number {
  return ~a;
}
console.log(shl(1024, 30));
console.log(shl(1, 32));
console.log(shr(-8, 1));
console.log(ushr(-1, 0));
console.log(band(123456789012, -1));
console.log(bnot(4294967295));

let acc: number = 0;
let i: number = 0;
while (i < 40) {
  acc = acc + (1 << i);
  i = i + 1;
}
console.log(acc);
