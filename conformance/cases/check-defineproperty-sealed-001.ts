// RFC C5b-4 — Object.defineProperty(O, newKey, desc) on a sealed /
// non-extensible dict throws TypeError per spec §10.1.6.3. Existing-
// key redefine that does not flip non-configurable flags is still
// allowed (e.g. updating writable=true entry's [[Value]]).
//
// Boolean `threw` flag pattern mirrors C4b's check-defineproperty-typeerror-001 —
// avoids `e.message` (tora's TypeError currently leaves .message =
// undefined; orthogonal pre-existing gap, tracked separately).

const o: any = { a: 1 };
Object.seal(o);

let threw_new_on_sealed = false;
try {
  Object.defineProperty(o, "b", { value: 2 });
} catch (e) {
  threw_new_on_sealed = true;
}
console.log("seal-then-define new key threw:", threw_new_on_sealed);

let threw_redefine_value = false;
try {
  Object.defineProperty(o, "a", { value: 99 });
} catch (e) {
  threw_redefine_value = true;
}
console.log("seal-then-redefine value threw:", threw_redefine_value);
console.log("o.a after redefine:", o.a);

const p: any = {};
Object.preventExtensions(p);

let threw_new_on_prevent = false;
try {
  Object.defineProperty(p, "x", { value: 1 });
} catch (e) {
  threw_new_on_prevent = true;
}
console.log("prevent-then-define new key threw:", threw_new_on_prevent);

// A fresh extensible dict still defines normally.
const q: any = {};
Object.defineProperty(q, "k", {
  value: 42,
  enumerable: true,
  configurable: true,
  writable: true,
});
console.log("fresh define new key:", q.k);
