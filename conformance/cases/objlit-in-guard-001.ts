// `in` on a cell that lacks a declared member walks the user
// [[Prototype]] a re-parent gave it; a cell nothing re-parented still
// answers false.
type P = { a?: number; b: number };
const plain: P = { b: 1 };
const moved: P = { b: 2 };
Object.setPrototypeOf(moved, { a: 9 });
console.log("a" in plain, "a" in moved);
const viaReflect: P = { b: 3 };
console.log(Reflect.setPrototypeOf(viaReflect, { a: 1 }), "a" in viaReflect);
const nulled: P = { b: 4 };
Object.setPrototypeOf(nulled, null);
console.log("a" in nulled);
const chain: P = { b: 5 };
Object.setPrototypeOf(chain, Object.create({ a: 2 }));
console.log("a" in chain, "a" in plain);
const back: P = { b: 6 };
Object.setPrototypeOf(back, Object.prototype);
console.log("a" in back);

class K {
  a?: number = 1;
  b = 2;
}
const k = new K();
delete k.a;
console.log("a" in k);
Object.setPrototypeOf(k, { a: 3 });
console.log("a" in k);
