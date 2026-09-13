// A top-level `const R = /…/` read from named function bodies: the
// regex literal's slot shape is a RegExp (fresh per evaluation,
// §13.2.7.3), so the binding promotes to the named-fn-visible global
// lane like a string or symbol literal does. The test262 harness's
// `__t262_ASCII_IDENTIFIER` is exactly this shape.
const R = /^[$_a-zA-Z][$_a-zA-Z0-9]*$/u;
function isIdent(s: string): boolean { return R.test(s); }
function keys(o: any): string[] {
  const out: string[] = [];
  for (const k of Object.keys(o)) { if (R.test(k)) out.push(k); }
  return out;
}
class C { static pick(s: string): string { return R.test(s) ? s : "-"; } }
console.log("regex-global", isIdent("abc"), isIdent("1x"), keys({ a: 1, "b-c": 2, $d: 3 }).join(","), C.pick("ok"), C.pick("9"));
