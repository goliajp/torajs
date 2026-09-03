// `String(q)` on a pointer-shaped `T | null` was refused outright,
// and the spellings that got past the refusal answered with the wrong
// thing: a pointer-shaped nullable is spelled as bare T all the way
// down, so every arm of the coercion read its in-band 0 as a live
// value of its own type. The Str arm handed the null pointer back —
// it printed as `null` and had no `.length` — and the array join
// kernel read it as a cell and produced nothing at all.
//
// §7.1.17 asks null first and ToString second.

const s: string | null = null;
console.log(String(s));
console.log(String(s).length);
console.log(String(s) === "null");

const s2: string | null = "ab";
console.log(String(s2), String(s2).length);

type O = { x: number };
const q: O | null = null;
console.log(String(q), String(q).length);
const q2: O | null = { x: 1 };
console.log(String(q2));

const a: number[] | null = null;
console.log(String(a), String(a).length);
const a2: number[] | null = [1, 2];
console.log(String(a2));
const a3: string[] | null = ["p", "q"];
console.log(String(a3));

type F = (n: number) => number;
const f: F | null = null;
console.log(String(f), String(f).length);
const f2: F | null = (n) => n;
console.log(String(f2).length > 0);

// the two scalars ride the any lane and always answered this
const n: number | null = null;
console.log(String(n), String(n).length);
const n2: number | null = 7;
console.log(String(n2));
const b: boolean | null = null;
console.log(String(b));

// a slot written both ways
let m: number[] | null = null;
console.log(String(m));
m = [3];
console.log(String(m));

// template substitution takes the same coercion
const t: string | null = null;
console.log(`v=${t}`);
const t2: string | null = "z";
console.log(`v=${t2}`);
const ta: number[] | null = null;
console.log(`v=${ta}`);

// non-nullable spellings are untouched
const p: number[] = [1, 2];
console.log(String(p), `${p}`);
const r: string = "k";
console.log(String(r), String(r).length);
const o: O = { x: 9 };
console.log(String(o));
