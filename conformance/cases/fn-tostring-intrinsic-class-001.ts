// §20.2.3.5 for a builtin constructor read as a value. tr injects the
// builtin classes, so they are registered classes with no span into
// the user program — the source table misses on them by design, and
// the branch that miss lands on is the native form, not an empty
// string.
console.log(String(Error));
console.log(String(TypeError));
console.log(String(RangeError));
const m: any = Map;
console.log(String(m));
const w: any = WeakMap;
console.log(String(w));
const s: any = Set;
console.log(String(s));
console.log((Error as any).toString());
console.log((TypeError as any).toLocaleString());

// Control — a class and a function the user wrote keep their own
// recorded text and must NOT fall to the native form. The text
// itself is not compared here: tr answers the source verbatim while
// bun reprints it, which is a separate, recorded difference.
class C {
  m(): number {
    return 1;
  }
}
console.log(String(C as any).indexOf("native code") >= 0);
function g(): number {
  return 1;
}
console.log(String(g).indexOf("native code") >= 0);
