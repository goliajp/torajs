// Phase 1c.4: zero-width assertions — positive `(?=X)` and negative
// `(?!X)` lookahead. Lookbehind `(?<=)` / `(?<!)` is Phase 1c.4.b
// (needs reverse matcher).

console.log(/.(?=Z)/.exec("a bZ cZZ")[0]);
console.log(/[a-e](?!Z)/.exec("aZ b")[0]);
console.log(/[a-e](?!Z)+/.exec("aZZZZ bZZZ cZZ dZ e")[0]);
console.log(/Java(?!Script)([A-Z]\w*)/.exec("JavaCorp")[0]);
console.log(/.(?=Z){2}/.exec("a bZ cZZ dZZZ eZZZZ")[0]);

console.log(/x(?=y)/.test("xy"));
console.log(/x(?=y)/.test("xz"));
console.log(/x(?!y)/.test("xy"));
console.log(/x(?!y)/.test("xz"));

console.log(/foo(?=bar)/.test("foobar"));
console.log(/foo(?=bar)/.test("foobaz"));

// Lookahead at end
console.log(/abc(?=$)/.test("abc"));
console.log(/abc(?=$)/.test("abcd"));

// Lookahead with replace
console.log("abc 123".replace(/\d+(?=$)/, "X"));
