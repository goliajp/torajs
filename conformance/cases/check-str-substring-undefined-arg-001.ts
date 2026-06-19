// S221 — String.substring(start, end) accepts Undefined for either
// positional per ES §22.1.3.22 step 4/5: start=undef → 0,
// end=undef → length.
console.log("abcdef".substring(2, undefined));
console.log("abcdef".substring(undefined, 4));
console.log("abcdef".substring(undefined, undefined));
console.log("abcdef".substring(1, undefined));
console.log("abcdef".substring(undefined, 1));
console.log("".substring(undefined, undefined));
console.log("x".substring(undefined, undefined));
console.log("abcdef".substring(2, undefined).length);
