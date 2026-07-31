// Reflect.getOwnPropertyDescriptor — §28.1.5 (strict IsObject, no ToObject)
const o = { a: 1 };
const d = Reflect.getOwnPropertyDescriptor(o, "a");
console.log(d.value, d.writable, d.enumerable, d.configurable);
console.log(Reflect.getOwnPropertyDescriptor(o, "missing"));

// primitive targets throw TypeError (unlike Object.getOwnPropertyDescriptor)
try {
  Reflect.getOwnPropertyDescriptor("abc" as any, "length");
} catch (e) {
  console.log("caught", e instanceof TypeError);
}
try {
  Reflect.getOwnPropertyDescriptor(1 as any, "x");
} catch (e) {
  console.log("caught2", e instanceof TypeError);
}
try {
  Reflect.getOwnPropertyDescriptor(null as any, "x");
} catch (e) {
  console.log("caught3", e instanceof TypeError);
}

// reflection face
console.log(typeof Reflect.getOwnPropertyDescriptor, Reflect.getOwnPropertyDescriptor.length);

// array length via Reflect
const arr = [1, 2, 3];
const dl = Reflect.getOwnPropertyDescriptor(arr, "length");
console.log(dl.value, dl.writable, dl.enumerable, dl.configurable);
