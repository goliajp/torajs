// §19.2.4 step 1 (parseFloat), §19.2.6.x step 1 (the URI four) and
// §B.2.1 step 1 (escape / unescape) all begin with ToString, which
// takes any value. `parseInt` lost its shape guard for this reason
// in rotation 461; these were the same sentence spelled a second
// time, and only the first spelling had been corrected.
//
// The lowering side needed nothing: `decode_any_to_str` already
// boxes whatever it gets, which is what made the guard a refusal
// with no counterpart in the runtime.
console.log(parseFloat(1.5));
console.log(parseFloat(true));
console.log(parseFloat(null));

console.log(escape(null));
console.log(escape(1n));
console.log(unescape(null));

console.log(encodeURI(123));
console.log(decodeURIComponent(456));

// ToString on an object runs its `toString`, which is why the guard
// could not have been a cheap approximation of the spec: the answer
// depends on user code.
const o = { toString(): string { return "7.5x"; } };
console.log(parseFloat(o));
console.log(escape(o));
