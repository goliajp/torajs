// 559-01 / 559-02 — a property key reaches the runtime as a Str whose
// payload is Latin-1 or UTF-16 code units, while a struct layout
// spells its field names in WTF-8 and a dynobj hashes its keys.
// Before: `o[k]` on a struct answered undefined for every non-ASCII
// key (payload bytes compared against WTF-8 name bytes), and a
// dynobj hashed / compared only the low byte of each UTF-16 unit
// (`d["Ā"]` read the `"Ȁ"` slot; `d["\uD800"]` the
// `"\uDC00"` one).
const hi = "\uD800";
const lo = "\uDC00";
const o = { "\uD800": 1, "\uDC00": 2, "é": 3, "中": 4, "\u{1f600}": 5 };
console.log(o["\uD800"], o["\uDC00"], o["é"], o["中"], o["\u{1f600}"]);
console.log(o[hi], o[lo], o["e" + "́"], o["中"], o["\uD83D" + "\uDE00"]);
console.log(o["\uDC00\uD800"], (o as any)["e"], (o as any)["é "]);
const k = ["\uD800", "\uDC00", "é", "中", "\u{1f600}", "x"];
console.log(k.map((n) => (o as any)[n]).join(","));
console.log(o.hasOwnProperty("é"), o.hasOwnProperty("\uD800"), o.hasOwnProperty("\uDBFF"));

// The same keys through a struct typed as `any` and a dynobj.
const a: any = o;
console.log(a["\uD800"], a["é"], a["中"], a[lo]);
const d: any = { "Ā": 1, "Ȁ": 2, "\uD800": "x", "\uDC00": "y", "é": "e" };
console.log(d["Ā"], d["Ȁ"], d["\uD800"], d["\uDC00"], d["é"], d["ā"]);
d["\uD83D"] = "h";
console.log(d["\uD83D"], d["\uDE00"], Object.keys(d).length);

// A pattern spelled with such a key reads it.
const { "\uD800": first, "é": e } = o;
console.log(first, e);

// Spread copies the layout, and reads find the copy's keys too.
const s = { ...o, c: 6 };
console.log(s["\uDC00"], s["中"], s.c);

// Writes through a runtime key land on the right slot.
const w: any = { "\uD800": 0, "\uDC00": 0 };
w[hi] = 7;
w["\uDC00"] = 8;
console.log(w["\uD800"], w[lo], JSON.stringify(w));
