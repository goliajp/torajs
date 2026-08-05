// Array spread of a string yields Unicode code points, not code units:
// §13.2.4.1 runs the spread through GetIterator, and the String
// iterator (§22.1.5) steps by code point.
//
// Pre-fix `[..."👋a"]` had three elements — the spread lowered to
// `split("")`, which is code-unit-correct for `String.prototype.split`
// and wrong here, so a surrogate pair came back as its two halves.
// `for (const c of s)` and `Array.from(s)` were already right, which
// is why this needed both a non-BMP string and the spread spelling to
// show up.

const wave = "👋a";

// .length stays in code units — that part was never wrong
console.log(wave.length);

// the three iteration spellings must now agree
console.log([...wave].length);
console.log(Array.from(wave).length);
let n = 0;
for (const c of wave) n += 1;
console.log(n);

// a lone astral character is one element, two code units
console.log([..."👋"].length, "👋".length);

// an emoji with a modifier is two code points
console.log([..."👋🏽"].length);

// BMP text is unaffected, including Latin-1 above ASCII
console.log([..."abc"].join("-"));
console.log([..."héllo"].join("|"));
console.log([...""].length);

// astral characters mixed with BMP neighbours keep their order
console.log([..."a👋b"].join(","));

// spread composes with literal elements on both sides
console.log([..."a", "z"].join(","));
console.log(["p", ..."qr"].join(","));

// a Substr view (slice) spreads the same way
const sub = "hello".slice(1, 3);
console.log([...sub].join("-"), sub.length);

// non-string spreads are untouched
console.log([...[1, 2], 3].join(","));
console.log([...new Set([1, 1, 2])].join(","));
const m = new Map([["k", 1]]);
console.log([...m.keys()].join(","));
