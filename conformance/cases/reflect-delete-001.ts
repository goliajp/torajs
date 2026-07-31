// Reflect.deleteProperty — §28.1.3 (strict IsObject + OrdinaryDelete boolean)
const o: any = { a: 1, b: 2 };
console.log(Reflect.deleteProperty(o, "a"));
console.log(o.a, o.b);
// absent key deletes to true per §10.1.10
console.log(Reflect.deleteProperty(o, "missing"));

// non-configurable refusal answers false
const locked: any = {};
Object.defineProperty(locked, "k", { value: 7, configurable: false });
console.log(Reflect.deleteProperty(locked, "k"), locked.k);

// primitive targets throw TypeError
try {
  Reflect.deleteProperty("abc" as any, "length");
} catch (e) {
  console.log("caught", e instanceof TypeError);
}
try {
  Reflect.deleteProperty(1 as any, "x");
} catch (e) {
  console.log("caught2", e instanceof TypeError);
}

// reflection face
console.log(typeof Reflect.deleteProperty, Reflect.deleteProperty.length);
