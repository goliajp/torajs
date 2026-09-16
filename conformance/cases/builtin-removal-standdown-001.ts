// A REMOVAL is a write. `delete Object.keys` makes `Object.keys(o)` a
// TypeError and `delete Array.prototype[Symbol.iterator]` makes
// `[...arr]` one — and the routes that fold those operations off the
// callee's syntax, or read the backing store, answered as if nothing
// had happened. Both censuses count a delete alongside an assignment.

const o = { a: 1 };
delete (Object as any).keys;
try {
  console.log("keys", JSON.stringify(Object.keys(o)));
} catch (e) {
  console.log("keys threw", (e as any).name);
}
delete (Math as any).max;
try {
  console.log("max", Math.max(1, 2));
} catch (e) {
  console.log("max threw", (e as any).name);
}

const a = [1, 2, 3];
delete (a as any)[Symbol.iterator];
// the own slot was never there; the prototype's still is
console.log("own-delete", JSON.stringify([...a]));
delete (Array.prototype as any)[Symbol.iterator];
try {
  console.log("spread", JSON.stringify([...a]));
} catch (e) {
  console.log("spread threw", (e as any).name);
}
try {
  for (const x of a) {
    console.log("fo", x);
  }
} catch (e) {
  console.log("for-of threw", (e as any).name);
}
