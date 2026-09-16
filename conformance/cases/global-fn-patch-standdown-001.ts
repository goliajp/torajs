// A bare global function is folded twice — once as a call route and
// once as an interned cell for the value read — and the global
// object's own bag reaches neither. `globalThis.parseInt = f` was a
// loud "not yet supported" for exactly that reason. It is supported
// now: a write to one of these names routes both the call and the
// value read through the object, which is where the write landed.

globalThis.parseInt = function (): number {
  return 42;
} as any;

console.log("call", parseInt("7"));
const q: any = parseInt;
console.log("value", q("7"));
console.log("member", (globalThis as any).parseInt("7"));

// a name nobody wrote keeps its fold
console.log("untouched", isNaN(1), parseFloat("2.5"), encodeURIComponent("a b"));

// the removal spelling too
delete (globalThis as any).isFinite;
try {
  console.log("isFinite", isFinite(1));
} catch (e) {
  console.log("isFinite threw", (e as any).name);
}
