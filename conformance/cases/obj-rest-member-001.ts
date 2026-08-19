// A member read on an object-rest binding that misses the static
// anchor is a runtime [[Get]] (§10.1.8.1) — excluded keys and
// never-present keys answer undefined; anchor keys keep their value.
const { a, b, ...rest } = { x: 1, y: 2, a: 5, b: 3 };
console.log("used", a, b);
// @ts-ignore
console.log("miss-a", rest.a);
// @ts-ignore
console.log("miss-b", rest.b);
console.log("hit-x", rest.x);
console.log("hit-y", rest.y);
// @ts-ignore
console.log("miss-z", rest.z);
