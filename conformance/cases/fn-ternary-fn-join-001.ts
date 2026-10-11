// a conditional expression whose branches are functions of different
// signatures (a generic function among them, two named functions with
// different returns, a nullable fn-typed param against a function) is
// one of them at run time and calls the chosen one
function decode(s) {
  return s.length;
}
function inc(n: number) {
  return n + 1;
}
function len(s: string) {
  return s.length;
}
function parse(o?: (s: string) => number) {
  const dec = o ? o : decode;
  return dec("abc");
}
console.log(parse(), parse((s) => 7));
function label(n: number) {
  return "#" + n;
}
function pick(c: boolean) {
  const f = c ? inc : label;
  return f(4);
}
console.log(pick(true), pick(false));
console.log((true ? inc : label)(1), (false ? inc : label)(1));
function pick2(o: ((s: string) => number) | null, c: boolean) {
  const f = c ? o : decode;
  return f === null ? -1 : f("ab");
}
console.log(pick2(null, true), pick2((s) => 9, true), pick2(null, false));
console.log(decode("xy"));
