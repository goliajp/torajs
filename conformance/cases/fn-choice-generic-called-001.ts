// a generic function (unannotated parameter) chosen as a value and then
// called: `o || decode`, `o ?? decode`, and the any-valued
// `opts?.decode || decode`, with and without a direct call of the
// function elsewhere
function decode(s) {
  return s.length;
}
function parseOr(o?: (s: string) => number) {
  const dec = o || decode;
  return dec("abc");
}
function parseNullish(o?: (s: string) => number) {
  const dec = o ?? decode;
  return dec("abcd");
}
function parseAny(s, opts) {
  const dec = opts?.decode || decode;
  return dec(s);
}
console.log(parseOr(), parseOr((s) => 7));
console.log(parseNullish(), parseNullish((s) => 8));
console.log(parseAny("abcde"), parseAny("abcde", { decode: (s) => s + "!" }));
console.log(decode("xy"));

function upper(s) {
  return s.toUpperCase();
}
function pick(f?: (s: string) => string) {
  const g = f || upper;
  return g("q");
}
console.log(pick(), pick((s) => s + s));
