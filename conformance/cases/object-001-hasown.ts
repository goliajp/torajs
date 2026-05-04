// v0.2 #3: Object.hasOwn — compile-time resolved when key is a Str literal
// and obj is statically a struct.

type Pt = { x: number, y: number };
const p: Pt = { x: 1, y: 2 };

console.log(Object.hasOwn(p, "x"));
console.log(Object.hasOwn(p, "y"));
console.log(Object.hasOwn(p, "z"));

type Person = { name: string, age: number, email: string };
const u: Person = { name: "Alice", age: 30, email: "a@example.com" };
console.log(Object.hasOwn(u, "name"));
console.log(Object.hasOwn(u, "age"));
console.log(Object.hasOwn(u, "phone"));
