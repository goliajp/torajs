// 557-02 — an object-literal key is a UTF-16 code-unit sequence
// (§6.1.7): `{ "\uD800": 1, "\uDC00": 2 }` has TWO keys, and neither
// collapses into U+FFFD in the layout, `Object.keys` / `values` /
// `entries`, `JSON.stringify` (§25.5.2.3 spells a lone surrogate as
// `\ud800`), `in`, or a spread copy. Reads through a runtime key on a
// struct / any receiver are the 559-01 / 559-02 runtime fixtures.
const hi = "\uD800";
const lo = "\uDC00";
const o = { "\uD800": 1, "\uDC00": 2 };
const ks = Object.keys(o);
console.log(ks.length, ks[0] === hi, ks[1] === lo, ks[0] === "�");
console.log(ks[0].charCodeAt(0).toString(16), ks[1].charCodeAt(0).toString(16));
console.log(JSON.stringify(o));
console.log("\uD800" in o, "\uDC00" in o, "�" in o, "𐀀" in o);
console.log(Object.values(o).join(","));
console.log(Object.entries(o).map(([k, v]) => k.charCodeAt(0).toString(16) + "=" + v).join(","));

// Mixed with identifier keys — declaration order survives.
const m = { a: 1, "\uD800": 2, b: 3 };
console.log(Object.keys(m).map((k) => k.charCodeAt(0).toString(16)).join(","));
console.log(m.a + m.b, JSON.stringify(m));

// Runtime-typed receiver: the same literal through the dynobj lane.
const d: any = { "\uD800": "x", "\uDC00": "y" };
console.log(Object.keys(d).length, JSON.stringify(d), lo in d);

// Spread copies the key as-is.
const s = { ...o, c: 3 };
console.log(Object.keys(s).length, JSON.stringify(s));
