// `String.prototype.at(index?)` per ES §22.1.3.1 step 2-3: the
// index argument routes through ToIntegerOrInfinity, where
// `undefined` → 0, so `s.at()` returns `s[0]` (a one-char string,
// matching the Substr-view emit). Pre-fix tora required strictly
// 1 arg and rejected the 0-arg form with "expected 1 argument(s),
// got 0"; bun / v8 accept it.

console.log("at():", "hello".at());
console.log("at(0):", "hello".at(0));
console.log("at(-1):", "hello".at(-1));
console.log("at(2):", "hello".at(2));

const s = "world";
console.log("var at():", s.at());
console.log("var at(-1):", s.at(-1));

// Substr-view receiver (after .slice) — same default arity path.
console.log("slice.at():", "hello".slice(1).at());
