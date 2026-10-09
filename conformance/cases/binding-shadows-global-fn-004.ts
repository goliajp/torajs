// A top-level declaration named like a global function property binds
// the name from the first statement on (§9.1.1.4): a call ahead of a
// `let` / `const` reaches it in its dead zone (§8.1.1.1, a
// ReferenceError), and a class of that name is what a bare call reaches
// (§15.7.14: calling it throws a TypeError).

try {
  console.log(escape("a b"));
} catch (e) {
  console.log((e as Error).name);
}
function viaFn(): string {
  return unescape("%41");
}
try {
  console.log(viaFn());
} catch (e) {
  console.log((e as Error).name);
}
let escape = (s: string): string => "mine:" + s;
const unescape = (s: string): string => "un:" + s;
console.log(escape("a b"), viaFn(), isNaN(1));

class parseInt {}
try {
  console.log((parseInt as any)("1"));
} catch (e) {
  console.log((e as Error).name);
}
console.log(parseFloat("2.5"), typeof parseInt);
