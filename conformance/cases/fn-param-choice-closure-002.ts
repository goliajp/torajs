// No caller hands these parameters a closure: only the choice inside
// the body puts one in their slot.
function lenOr(s: string, o?: (s: string) => number): number {
  const dec = o || ((x: string): number => x.length);
  return dec(s);
}
function lenNul(s: string, o?: (s: string) => number): number {
  const dec = o ?? ((x) => x.length * 1.5);
  return dec(s);
}
function lenAnd(s: string, o?: (s: string) => number): number {
  const dec = (o && ((x: string): number => x.length + 100)) || ((x: string) => x.length);
  return dec(s);
}
function twice(s: string): number {
  return s.length * 2;
}
console.log(lenOr("abc"), lenNul("abc"), lenAnd("abc"));
console.log(lenOr("abc", twice), lenNul("abc", twice), lenAnd("abc", twice));
