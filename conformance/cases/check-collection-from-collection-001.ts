// S166 — `new Map(<Map>)` / `new Set(<Set>)` copy-ctor per ES §24.{1,2}.1.
// Map clones (k, v) with both heap key/value rc_inc; Set reuses
// __torajs_set_union(src, NULL) which walks src + skips NULL other.

// 1) Map<string, number> copy: size + per-key get + iter shape.
const m1 = new Map([["k", 99], ["x", 42]]);
const m1c = new Map(m1);
console.log(m1c.size);
console.log(m1c.get("k"));
console.log(m1c.get("x"));
for (const [k, v] of m1c) {
  console.log("m1c-entry", k, v);
}

// 2) Map<string, string> copy: heap-key + heap-value rc_inc paths.
const m2 = new Map([["a", "alpha"], ["b", "bravo"]]);
const m2c = new Map(m2);
console.log(m2c.size, m2c.get("a"), m2c.get("b"));

// 3) Src + clone independent after clone (mutating src shouldn't touch clone).
const m3 = new Map([["k", 1], ["x", 2]]);
const m3c = new Map(m3);
m3.delete("k");
m3.set("y", 3);
console.log(m3.size, m3.get("k"), m3.get("y"));
console.log(m3c.size, m3c.get("k"), m3c.get("y"));

// 4) Empty Map copy.
const m4: Map<string, number> = new Map();
const m4c = new Map(m4);
console.log(m4c.size);

// 5) Set<number> copy: size + per-elem has + iter shape.
const s1 = new Set([1, 2, 3]);
const s1c = new Set(s1);
console.log(s1c.size, s1c.has(1), s1c.has(2), s1c.has(3), s1c.has(4));
for (const v of s1c) {
  console.log("s1c-elem", v);
}

// 6) Set<string> copy: heap-key rc_inc path.
const s2 = new Set(["alpha", "bravo", "charlie"]);
const s2c = new Set(s2);
console.log(s2c.size, s2c.has("alpha"), s2c.has("bravo"), s2c.has("delta"));

// 7) Src + clone independent after clone.
const s3 = new Set([1, 2, 3]);
const s3c = new Set(s3);
s3.delete(2);
s3.add(99);
console.log(s3.size, s3.has(2), s3.has(99));
console.log(s3c.size, s3c.has(2), s3c.has(99));

// 8) Empty Set copy.
const s4: Set<number> = new Set();
const s4c = new Set(s4);
console.log(s4c.size);
