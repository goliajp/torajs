// `in` on struct / class-instance receivers walks the full chain:
// own layout (fields, objlit accessor slots) -> class prototype
// (methods + accessor halves; NOT own -- hasOwnProperty says false
// while `in` says true) -> Object.prototype root. The static fold
// only proves presence; absence and dynamic keys route through the
// runtime kernels (which also retired the literal-key-only panic).

class C {
  x = 1;
  m(): number {
    return 2;
  }
  get g(): number {
    return 3;
  }
}
const c = new C();
console.log("x" in c); // true (own field, static fold)
console.log("m" in c); // true (class prototype method)
console.log("g" in c); // true (class accessor)
console.log("toString" in c); // true (Object.prototype)
console.log("hasOwnProperty" in c); // true
console.log("constructor" in c); // true
console.log("nope" in c); // false

const ca: any = c;
console.log("m" in ca); // true (same chain through any)
console.log("g" in ca); // true
console.log("toString" in ca); // true
console.log("nope" in ca); // false

const p = { x: 1 };
console.log("x" in p); // true (own, static fold)
console.log("toString" in p); // true (chain root)
console.log("hasOwnProperty" in p); // true
console.log("nope" in p); // false
