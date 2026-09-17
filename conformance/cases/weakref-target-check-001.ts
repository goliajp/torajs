// §26.1.1.1 step 2 — CanBeHeldWeakly(target): objects and symbols not
// in the global registry; everything else is a TypeError.
const bad: any[] = [1, undefined, null, "s", true, 10n, Symbol.for("registered")];
for (const t of bad) {
  try {
    new WeakRef(t);
    console.log("no throw", typeof t);
  } catch (e: any) {
    console.log(e.constructor.name, e.message);
  }
}
const obj = { k: 1 };
const sym = Symbol("local");
const fn = () => 1;
console.log(new WeakRef(obj).deref() === obj, new WeakRef(sym).deref() === sym, new WeakRef(fn).deref() === fn);
const n: number = 3;
try {
  new WeakRef(n as any);
} catch (e: any) {
  console.log("typed number", e.constructor.name);
}
