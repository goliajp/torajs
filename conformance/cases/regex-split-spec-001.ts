// String.prototype.split with a regexp separator -- ES 22.2.6.14
// RegExpSplit: p/q segment protocol, an empty match adjacent to
// the segment start never splits, exec only runs while q < size,
// and a zero-length subject answers [] iff the separator matches
// the empty string. Plus the lazy-quantifier DFA gate (a lazy
// match end depends on thread priority, which the DFA powerset
// erases -- /.*?/ answered the greedy boundary).
// RFC 20260712-string-proto-cluster chunk D.
console.log(JSON.stringify("x".split(/^/)));
console.log(JSON.stringify("x".split(/$/)));
console.log(JSON.stringify("x".split(/.?/)));
console.log(JSON.stringify("x".split(/.*/)));
console.log(JSON.stringify("x".split(/.*?/)));
console.log(JSON.stringify("x".split(/()/)));
console.log(JSON.stringify("x".split(/(?:)/)));
console.log(JSON.stringify("ab".split(/(?:)/)));
console.log(JSON.stringify("aXb".split(/(X)/)));
console.log(JSON.stringify("".split(/^/)));
console.log(JSON.stringify("".split(/x/)));
console.log(JSON.stringify("She sells seashells".split(/s/)));
console.log(JSON.stringify("a1b2c".split(/[0-9]/)));
console.log(JSON.stringify("x".split(/.{1}/)));
console.log(JSON.stringify("hello".split(/l+/)));

// lazy quantifiers route Pike-only.
console.log(JSON.stringify(/.*?/.exec("x")));
console.log(JSON.stringify("xaab".match(/a+?/)));
console.log(JSON.stringify("begin end".replace(/e.*? /, "")));
console.log(/x??/.test("y"));
