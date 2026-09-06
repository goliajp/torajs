// §B.2.1 — the two Annex B string globals, missing until now: every
// program naming them died at compile time on an unknown identifier.
//
// Both walk CODE UNITS, not code points, which is why neither can be
// written in terms of the URI encoder next door: that one combines a
// surrogate pair into UTF-8 and raises URIError on a lone surrogate,
// and Annex B does neither.

// §B.2.1.1 step 4.b — the set written through verbatim.
console.log(escape("abcXYZ098@*_+-./"));

// Below 256 is `%XX`, at or above is `%uXXXX`, hex uppercase.
console.log(escape(" ~ÿĀ中"));

// A surrogate pair is two independent escapes.
console.log(escape("\u{1D306}"));

// §B.2.1.2 inverts both forms, and lowercase hex is accepted.
console.log(unescape("%20%7E%FF%u0100%u4E2D"));
console.log(unescape("%ff"), unescape("%uD834%uDF06") === "\u{1D306}");

// What it cannot parse it keeps, and the scan continues after it.
console.log(unescape("%"), unescape("%2"), unescape("%zz"), unescape("%u12"));
console.log(unescape("%u123z"), unescape("a%b%20c"));

// ToString runs first (step 1), and the empty string round-trips.
console.log(escape(123 as any), unescape(456 as any));
console.log(JSON.stringify(escape("")), JSON.stringify(unescape("")));

// They are functions.
console.log(typeof escape, typeof unescape);

// The round trip holds over a mixed string.
const s = "a bé中\u{1D306}!";
console.log(unescape(escape(s)) === s);
