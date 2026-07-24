// Rotation 207 — §25.5.2.4 step 10: a BigInt has no JSON
// representation, so serializing one is a TypeError. It reached the
// any-lane walk's catch-all for classes with no own enumerable
// properties and answered `{}` instead — silently, at every position.

const x: any = 1n;
try {
  console.log("A no-throw", JSON.stringify(x));
} catch (e) {
  console.log("A", e instanceof TypeError);
}

const y: any = { v: 2n };
try {
  console.log("B no-throw", JSON.stringify(y));
} catch (e) {
  console.log("B", e instanceof TypeError);
}

const z: any = [3n];
try {
  console.log("C no-throw", JSON.stringify(z));
} catch (e) {
  console.log("C", e instanceof TypeError);
}

// Nested one level down still throws out of the whole call.
const w: any = { outer: { inner: 4n } };
try {
  console.log("D no-throw", JSON.stringify(w));
} catch (e) {
  console.log("D", e instanceof TypeError);
}

// The throw is catchable and leaves the walk usable afterwards.
const ok: any = { a: 1, b: "two" };
console.log("E", JSON.stringify(ok));
