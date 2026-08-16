// §27.5.1.5 / §27.6.1.5 — %GeneratorPrototype%[@@toStringTag] is
// "Generator" ("AsyncGenerator" for the async family), a real own
// entry {W:0,E:0,C:1}: the badge's step-15 [[Get]] and
// getOwnPropertyDescriptor must agree (420-05).
function* g() {
  yield 1;
}
async function* ag() {
  yield 1;
}

console.log(String(g()));
console.log(Object.prototype.toString.call(g()));
console.log(Object.prototype.toString.call(ag()));

const genProto = Object.getPrototypeOf(Object.getPrototypeOf(g()));
const desc = Object.getOwnPropertyDescriptor(genProto, Symbol.toStringTag);
console.log(desc?.value, desc?.writable, desc?.enumerable, desc?.configurable);
