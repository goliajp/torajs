// Rotation 543 — `box_to_tag_value`'s heap arms take the SLOT's stake
// (the rc_inc lives inside the shim, and its own comment says so), so
// an owned element temp still holds the one it was born with. The
// Map / Set literal initializer never gave that one back.
//
// 200k churn, AOT product RSS, 1.51 MB flat baseline:
//   new Map([["k" + i, "v" + i]])   14.63 MB -> 1.80 MB
//   new Set(["s" + i])               8.19 MB -> 1.80 MB
//
// The control that should have caught this hid it instead: the same
// spellings with LITERAL elements — `new Map([["k", "v"]])`,
// `new Set(["s"])` — measured 1.79 MB before the fix, because a
// static Str's rc traffic is a no-op and two cells cannot leak. Only
// an element minted fresh each iteration makes a stranded reference
// cost memory.
const m = new Map([
  ["a", 1],
  ["b", 2],
]);
console.log(m.get("a"), m.get("b"), m.size);
console.log([...m.entries()], [...m.keys()], [...m.values()]);

const k = "a" + 1;
const m2 = new Map([[k, "v"]]);
console.log(m2.get("a1"), k, m2.size);

const m3 = new Map([[1, [2, 3]]]);
console.log(m3.get(1), m3.size);

const m4 = new Map([["a", 1]]);
m4.set("b", 2);
console.log(m4.size, m4.get("a"), m4.get("b"));

const s = new Set(["x", "y", "x"]);
console.log(s.size, s.has("x"), [...s]);

const s2 = new Set([[1], [2]]);
console.log(s2.size, [...s2]);

const s3 = new Set(["a" + 1]);
console.log(s3.has("a1"), JSON.stringify([...s3]));

const s4 = new Set([1, 2]);
console.log(s4.size, s4.has(1));
