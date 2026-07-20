// §13.2.8.6 — a template substitution runs the IMPLICIT ToString, so
// a Symbol substitution throws TypeError (any-typed and statically
// typed alike), while the explicit String(sym) spelling keeps the
// §22.1.1 SymbolDescriptiveString. Non-symbol substitutions are
// unchanged (both kernels are hint-string).
const s: any = Symbol("d");
try {
  const t: string = `x-${s}`;
  console.log("any-no-throw", t);
} catch (e: any) {
  console.log("any threw", e instanceof TypeError);
}
const ts = Symbol("t");
try {
  const u: string = `y-${ts}`;
  console.log("typed-no-throw", u);
} catch (e: any) {
  console.log("typed threw", e instanceof TypeError);
}
console.log(`n-${1}`, `s-${"a"}`, `b-${true}`, `u-${undefined}`);
const obj: any = { toString: function () { return "T"; }, valueOf: function () { return "V"; } };
console.log(`o-${obj}`);
console.log(String(s));
console.log(s.toString());
