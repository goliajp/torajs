// 558-04 — a lone surrogate written to stdout / stderr comes out as
// U+FFFD: the stream is UTF-8 and a surrogate is not a scalar value.
// The string keeps its code unit (length / charCodeAt / JSON.stringify
// all see it); only the emitted bytes replace it.
const hi = "\uD83D";
const lo = "\uDE00";
const pair = hi + lo;
console.log(hi + "a");
console.log("a" + lo + "b" + hi);
console.log(pair, pair.length, (hi + "a").length, (hi + "a").charCodeAt(0));
console.log(JSON.stringify(hi + "a"), JSON.stringify(lo));
console.log(`t${hi}t`, [hi, lo].join("|"), (hi + "x").toUpperCase());
console.error(hi + "!" + lo);
console.log(String.fromCharCode(0xd800), String.fromCharCode(0xdfff, 0x41));
console.log((hi + "a").slice(0, 1) === hi, (hi + "a").slice(0, 1).length);
console.log("😀".slice(0, 1), "😀".slice(1), "😀".slice(0, 1) + "😀".slice(1));
