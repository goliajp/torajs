// `a || b` and `a && b` answer one of their operands, so a closure
// read out of one returns whatever width either operand returns: an
// integer-returning default and a fractional caller-supplied closure
// share the call site's reading.
function lenOr(s: string, o?: (s: string) => number): number {
  const dec = o || ((x: string): number => x.length);
  return dec(s);
}
function lenAnd(s: string, o?: (s: string) => number): number {
  const dec = (o && ((x: string): number => x.length * 100)) || ((x: string) => x.length / 4);
  return dec(s);
}
console.log(lenOr("abc"), lenOr("abc", (s) => s.length + 0.25), lenOr("abc", (s) => 7));
console.log(lenAnd("abc"), lenAnd("abc", (s) => 0.5));
