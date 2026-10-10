// A fn-typed parameter that shares a choice with a closure
// (`o || (s => s.length)`) holds whatever the choice's one slot holds:
// the closure's cell, or the cell of the function a caller passed.
function lenOr(s: string, o?: (s: string) => number): number {
  const dec = o || ((x: string): number => x.length);
  return dec(s);
}
function lenNul(s: string, o?: (s: string) => number): number {
  const dec = o ?? ((x) => x.length * 1.5);
  return dec(s);
}
function lenUni(s: string, o: ((s: string) => number) | undefined): number {
  const dec = o || ((x) => x.length);
  return dec(s);
}
function twice(s: string): number {
  return s.length * 2;
}
console.log(lenOr("abc"), lenOr("abcd"));
console.log(lenNul("abc"));
console.log(lenUni("ab", undefined));
console.log(lenOr("abc", twice), lenNul("abc", twice), lenUni("abc", twice));
console.log(lenOr("abc", (s) => s.length + 0.25), lenNul("abc", (s) => 7));
