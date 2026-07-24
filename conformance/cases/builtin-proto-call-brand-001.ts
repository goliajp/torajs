// Rotation 207 — `X.prototype.m.call(recv, ...)` is desugared to
// `recv.m(...)` at the AST level, which discards where the method
// came from. For the brand-checked wrapper methods (§21.1.3
// thisNumberValue, §20.3.3 thisBooleanValue, §22.1.3.28
// thisStringValue) that is observable: they must throw on a receiver
// of the wrong brand, while `recv.m()` answers the receiver's OWN m
// (a plain object badges itself). Those methods now skip the rewrite
// and take the reified-cell path, whose `.call` short-circuit runs
// the brand gate.

// Wrong brand throws.
try {
  console.log("A no-throw", Boolean.prototype.toString.call({}));
} catch (e) {
  console.log("A", e instanceof TypeError);
}
try {
  console.log("B no-throw", Number.prototype.toString.call({}));
} catch (e) {
  console.log("B", e instanceof TypeError);
}
try {
  console.log("C no-throw", String.prototype.toString.call({}));
} catch (e) {
  console.log("C", e instanceof TypeError);
}
try {
  console.log("D no-throw", Number.prototype.valueOf.call("nope"));
} catch (e) {
  console.log("D", e instanceof TypeError);
}
try {
  console.log("E no-throw", String.prototype.valueOf.call({}));
} catch (e) {
  console.log("E", e instanceof TypeError);
}
try {
  console.log("F no-throw", Boolean.prototype.valueOf.call(0));
} catch (e) {
  console.log("F", e instanceof TypeError);
}

// Right brand still dispatches, arguments included.
console.log("G", Number.prototype.toString.call(255, 16));
console.log("H", Number.prototype.valueOf.call(42));
console.log("I", Boolean.prototype.toString.call(false));
console.log("J", Boolean.prototype.valueOf.call(true));
console.log("K", String.prototype.toString.call("hi"));
console.log("L", String.prototype.valueOf.call("hi"));
console.log("M", Number.prototype.toFixed.call(3.14159, 2));

// Namespaces that already skipped the rewrite are unchanged.
console.log("N", Object.prototype.toString.call([]));
console.log("O", Array.prototype.slice.call([1, 2, 3], 1));
console.log("P", String.prototype.slice.call("abcdef", 1, 3));

// toLocaleString keeps the rewrite — the inherited generic is not
// brand-checked.
console.log("Q", Number.prototype.toLocaleString.call(42));
console.log("R", String.prototype.toLocaleString.call("hi"));
