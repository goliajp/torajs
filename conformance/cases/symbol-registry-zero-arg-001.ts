// §18 binds a missing argument to undefined, and both registry
// clauses have an answer for it: §20.4.2.4 step 1 is ToString(key),
// which makes `Symbol.for()` the registered symbol under the STRING
// "undefined"; §20.4.2.8 step 1 rejects a non-Symbol with a TypeError.
// Refusing either call at compile time was the static-subset answer.

const s = Symbol.for();
console.log("for", String(s), s === Symbol.for("undefined"));

try {
  Symbol.keyFor();
  console.log("keyFor no-throw");
} catch (e: any) {
  console.log("keyFor", e.constructor.name);
}

// The filled forms keep their own lanes.
console.log("for-str", String(Symbol.for("x")));
console.log("keyFor-sym", Symbol.keyFor(Symbol.for("y")));
console.log("keyFor-unregistered", Symbol.keyFor(Symbol("z")));
console.log("for-explicit-undefined", Symbol.for(undefined as any) === s);
