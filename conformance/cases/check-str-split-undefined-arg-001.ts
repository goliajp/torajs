// S215 — String.prototype.split(separator, undefined) per ES §22.1.3.21
// step 2: `If limit is undefined, lim = 2^32-1` — no truncation,
// returns the full split list.
console.log("a,b,c".split(",", undefined));
console.log("a,b,c,d,e".split(",", undefined));
console.log("hello".split("", undefined));
console.log("".split(",", undefined));
console.log("abc".split("x", undefined));
console.log("a,,b".split(",", undefined));
console.log("a,b,c".split(",", undefined).length);
