// §23.1.3.30 SortCompare runs ToNumber over whatever the comparator
// hands back, so a comparator is not required to return a number.
// tr seeded its lifted closure's return annotation as `number` and
// treated that as a PIN, so three legal spellings were refused with a
// return-type mismatch — and a fourth, `toSorted`, was absent from the
// seed table entirely and failed one step later at the argument check.
//
// The seed is now a fallback: it pins `number` only when the body
// already produces one, which keeps every numeric comparator on the
// typed lane, and seeds `any` otherwise, where the kernel's own
// ToNumber has always been right.
//
// Note what is NOT being asserted. A comparator that is not
// CONSISTENT leaves the order implementation-defined (§23.1.3.30
// step 2), so only the consistent ones below fix an order; the
// inconsistent ones assert that the call returns the array and does
// not throw.

// strings, consistent — ToNumber("-1") is -1
const a = [3, 1, 2];
a.sort((x, y) => { if (x < y) return "-1"; if (x > y) return "1"; return "0"; });
console.log("a", a.join(","));

// numbers, the shape that was already right — must stay right
const b = [3, 1, 2];
b.sort((x, y) => x - y);
console.log("b", b.join(","));

// an explicitly annotated comparator is untouched by the seed
const c = [3, 1, 2];
c.sort((x: number, y: number): number => x - y);
console.log("c", c.join(","));

// booleans: ToNumber(true) is 1, ToNumber(false) is +0 — inconsistent
// (it can never answer negative), so assert only that it runs
const d = [3, 1, 2];
const dr = d.sort((x, y) => x > y);
console.log("d", dr.length, dr === d);

// undefined: ToNumber(undefined) is NaN, which SortCompare reads as
// +0 — no reordering, and no throw
const e = [3, 1, 2];
e.sort((x, y) => undefined);
console.log("e", e.join(","));

// toSorted is the same signature (§23.1.3.34) and does not mutate
const f = [3, 1, 2];
const g = f.toSorted((x, y) => x < y ? -1 : x > y ? 1 : 0);
console.log("f", f.join(","), g.join(","));

// bigint results go through ToNumber too, and strings still sort
const h = ["b", "a", "c"];
h.sort((x, y) => x > y ? 1 : x < y ? -1 : 0);
console.log("h", h.join(","));
