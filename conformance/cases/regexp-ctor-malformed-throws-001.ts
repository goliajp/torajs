// §22.2.3.1 (rotation 576). A constant `new RegExp("…", "…")` is
// folded to a literal only when the pattern is well-formed: a
// malformed one has to stay a constructor call, so the SyntaxError is
// thrown where the spec throws it — at the call, catchable — and not
// at compile time. The fold and the compile-time reject pass now ask
// one function that question; when only the reject pass learned about
// `resolve_backrefs`, the fold handed it `RegExp("\\5", "u")` as a
// literal and the whole program stopped compiling.
function why(f: () => void): string {
  try { f(); return "no throw"; } catch (e: any) { return e.constructor.name; }
}
console.log(why(() => { new RegExp("[", "u"); }));
console.log(why(() => { new RegExp("\\5", "u"); }));
console.log(why(() => { new RegExp("(?<a>.)\\k<b>", ""); }));
console.log(why(() => { new RegExp("(?<42a>a)", ""); }));
console.log(why(() => { new RegExp("a", "gg"); }));
console.log(why(() => { new RegExp("a", "uv"); }));
// The well-formed ones still fold, and still work.
console.log(new RegExp("(?<x>a)", "g").test("a"));
console.log(RegExp("a", "g").test("a"));
console.log(new RegExp("\\5", "").test(""));
