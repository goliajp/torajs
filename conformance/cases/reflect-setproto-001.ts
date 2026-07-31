// Reflect.setPrototypeOf — §28.1.12 (strict IsObject + boolean answer)
const a: any = {};
const b: any = { greet: 1 };
console.log(Reflect.setPrototypeOf(a, b));
console.log(Reflect.getPrototypeOf(a) === b);
console.log(Reflect.setPrototypeOf(a, null));
console.log(Reflect.getPrototypeOf(a));

// cycle refusal answers false, no throw
const c1: any = {};
const c2: any = {};
Reflect.setPrototypeOf(c1, c2);
console.log(Reflect.setPrototypeOf(c2, c1));

// non-extensible target refusal answers false
const sealed: any = {};
Reflect.preventExtensions(sealed);
console.log(Reflect.setPrototypeOf(sealed, b));

// invalid proto throws TypeError (both flavors)
try {
  Reflect.setPrototypeOf(a, 1 as any);
} catch (e) {
  console.log("caught", e instanceof TypeError);
}
// primitive target throws TypeError
try {
  Reflect.setPrototypeOf("abc" as any, b);
} catch (e) {
  console.log("caught2", e instanceof TypeError);
}

// reflection face
console.log(typeof Reflect.setPrototypeOf, Reflect.setPrototypeOf.length);
