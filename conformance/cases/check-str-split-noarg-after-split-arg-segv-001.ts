// ES §22.1.3.21 step 4: when separator is undefined (the no-arg
// call `s.split()`), the result is a fresh Array containing the
// single string `S`. Previously tora's ssa_lower fell through to
// a 1-arg `Call(__torajs_str_split, [recv])` whose missing `sep`
// slot read register garbage — single-call programs survived (the
// garbage happened to be a walkable Str pointer) but any prior
// `.split(arg)` shifted residual register state and the next no-arg
// call SIGSEGV'd inside `str_view(sep)`.

// Direct no-arg call still works.
console.log("abc".split());
console.log("".split());

// Prior `.split("")` then `.split()` — was SEGV.
console.log("abc".split(""));
console.log("abc".split());

// Prior `.split(<arg>)` then `.split()` — was SEGV.
console.log("a,b,c".split(","));
console.log("a,b,c".split());

// Multiple no-arg calls in sequence.
console.log("hello".split());
console.log("world".split());

// Typeof + length parity.
const r1 = "abc".split("");
const r2 = "abc".split();
console.log(typeof r1, r1.length);
console.log(typeof r2, r2.length);
console.log(JSON.stringify(r2));
