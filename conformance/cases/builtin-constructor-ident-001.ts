// L3b 4 residue -- builtin .constructor reads answer the family's
// interned constructor cell, and the bare namespace ident (Object /
// Array / Number ...) reads as that same VALUE, so the identity
// compares hold. Locals shadow the builtin name.
const o: any = {};
console.log(o.constructor === Object);
const arr: any = [1, 2];
console.log(arr.constructor === Array);
const s: any = "hi";
console.log(s.constructor === String);
const n: any = 42;
console.log(n.constructor === Number);
const b: any = true;
console.log(b.constructor === Boolean);
function f(): number {
  return 1;
}
const fa: any = f;
console.log(fa.constructor === Function);
const m: any = new Map();
console.log(m.constructor === Map);
console.log(typeof Object);
console.log(Object === Array);
const shadowed = ((): boolean => {
  const Object = 5;
  return (Object as any) === 5;
})();
console.log(shadowed);
