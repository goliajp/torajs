// §22.1.3.19 step 5 — `functionalReplace` is IsCallable(replaceValue),
// and §7.2.3 names three shapes. Both of tr's answers read the cell
// tag instead, so a callable Proxy and a class object answered "not
// callable" and fell through to step 12's ToString leg.
//
// That is not a refusal. `"abcabc".replace("b", new Proxy(fn, {}))`
// SPLICED `function ProxyObject() { [native code] }` into the
// result — a wrong string, from a program that compiled and ran.
// No gate can see it: a fixture without a proxy replacement is
// byte-identical either way.
//
// The kernels want a Closure cell and the other two shapes have
// none, so a thunk is minted for them exactly as the array
// higher-order kernels do.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
const st = "abcabc";
const proxied: any = new Proxy((m: any) => "<" + m + ">", {});
const trapped: any = new Proxy((m: any) => m, {
  apply: (_t: any, _th: any, a: any) => "T" + a[0],
});
class K {}

console.log("literal proxy ", attempt(() => st.replace("b", proxied)));
console.log("replaceAll    ", attempt(() => st.replaceAll("b", proxied)));
// The handler decides the call, not the target (§10.5.12).
console.log("apply trap    ", attempt(() => st.replace("c", trapped)));
// A class object IS callable, so step 5 takes the functional leg
// and §10.2.1 step 2 refuses the call itself — a TypeError, where
// before the class's source text went into the string.
console.log("class repl    ", attempt(() => st.replace("b", K as any)));
// Everything not callable keeps step 12's ToString leg.
console.log("string repl   ", attempt(() => st.replace("b", "X")));
console.log("number repl   ", attempt(() => st.replace("b", 5 as any)));
console.log("object repl   ", attempt(() => st.replace("b", {} as any)));
console.log("undefined repl", attempt(() => st.replace("b", undefined as any)));
// The ordinary closure path is untouched, literal needle and regex.
console.log("fn repl       ", attempt(() => st.replace("b", (m: string) => "[" + m + "]")));
console.log("fn replaceAll ", attempt(() => st.replaceAll("b", (m: string) => "[" + m + "]")));
console.log("regex fn      ", attempt(() => st.replace(/(a)(b)/, (m: string, p1: string, p2: string) => p2 + p1)));
console.log("regex str     ", attempt(() => st.replace(/b/g, "Y")));
// Registered, not fixed here: `st.replace(/b/, anyValue)` is still
// refused at lowering ("repl must be Str or Closure, got Any") —
// that is the typed regex lane's own shape gate, a refusal rather
// than a wrong value, and a separate question from step 5.
