// `Array.prototype.at(index?)` per ES §23.1.3.1 step 2-3: the index
// argument routes through ToIntegerOrInfinity, where `undefined` →
// 0, so `a.at()` is equivalent to `a.at(0)` and returns the head
// element. Pre-fix tora typechecked `at` as strictly 1-arg and
// rejected the 0-arg form ("expected 1 argument(s), got 0"), even
// though bun / v8 accept it.

const a = [10, 20, 30];
console.log("at():", a.at());
console.log("at(0):", a.at(0));
console.log("at(2):", a.at(2));
console.log("at(-1):", a.at(-1));

const s: string[] = ["x", "y", "z"];
console.log("at() on str arr:", s.at());
console.log("at(-1) on str arr:", s.at(-1));

// Note: `[].at()` returning undefined (the OOB widen) is the
// known L3b carry on typed `Array<T>.at`/`{find,findIndex,...}` return
// type — out of scope for this narrow arity-only fix.
