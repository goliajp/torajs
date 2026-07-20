// any-lane `s.split(sep, limit)` — §22.1.3.23 step 4: limit's
// ToUint32 participates (was: limit silently ignored). Covers the
// regexp / string separator lanes, lim == 0 → [], NaN → 0, negative
// wrap (2^32-1: no truncation), and explicit undefined.
let a: any = new String("hello");
console.log(a.split(/l/, 2).join("|"));
console.log(a.split(/l/, 2).length);
console.log(a.split("l", 1).join("|"));
console.log(a.split(new RegExp("l"), 0).length);
console.log(a.split("l", NaN).length);
console.log(a.split("l", -1).join("|"));
console.log(a.split("l", undefined).join("|"));
let c: any = new String("one-two-three-four-five");
console.log(c.split("-", 2).join("|"));
console.log(c.split("-", 4294967295).join("|"));
console.log(c.split(undefined, 1).join("|"));
