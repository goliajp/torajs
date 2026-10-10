// a generic function used as a value only inside generic functions
// that are never called still compiles: its value wrapper is checked
// on its own
function len(s) {
  return s.length;
}
export function kind(x) {
  return typeof len + x;
}
export function asAny(x) {
  const f = len as any;
  return f(x);
}
console.log(len("abc"));
