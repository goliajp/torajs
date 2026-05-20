// Phase 1c.4.b: lookbehind assertions — positive `(?<=X)` and
// negative `(?<!X)`. Pair with regex-007-lookahead.ts which covers
// the lookahead side; this one drives the lookbehind matcher path.

// Positive lookbehind — fixed-width body
console.log(/(?<=foo)bar/.exec("foobar")[0]);
console.log(/(?<=foo)bar/.test("foobar"));
console.log(/(?<=foo)bar/.test("xxxbar"));

// Negative lookbehind — blocks match when preceding text matches body
console.log(/(?<!foo)bar/.exec("xxxbar")[0]);
console.log(/(?<!foo)bar/.test("xxxbar"));
console.log(/(?<!foo)bar/.test("foobar"));

// Lookbehind at start-of-string — body anchored to start
console.log(/(?<=^)abc/.exec("abc")[0]);
console.log(/(?<=^)abc/.test("abc"));
console.log(/(?<=^)abc/.test("xabc"));

// Lookbehind mid-pattern — interleaves with regular consumption
console.log(/x(?<=x)foo/.exec("xfoo")[0]);
console.log(/x(?<=x)foo/.test("xfoo"));

// Combined lookbehind + lookahead in one pattern
console.log(/(?<=foo)bar(?=baz)/.exec("foobarbaz")[0]);
console.log(/(?<=foo)bar(?=baz)/.test("foobarbaz"));
console.log(/(?<=foo)bar(?=baz)/.test("foobarqux"));
