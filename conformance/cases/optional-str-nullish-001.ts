// chunk 786 — `??` on a pointer-shaped optional field treats the
// per-type undefined sentinel cell as nullish (previously it only
// compared NULL, so a filled optional string field rode the non-null
// path and printed the sentinel text "undefined" instead of taking
// the fallback). || / === undefined / ternary exercise the already-
// landed truthiness and eq stations alongside.
type O = { tag?: string, n: number };
const o: O = { n: 5 };
console.log(String(o.n) + (o.tag ?? "-"));
console.log(o.tag || "fallback");
console.log(o.tag === undefined);
console.log(o.tag ? "yes" : "no");
const p: O = { tag: "x", n: 6 };
console.log(String(p.n) + (p.tag ?? "-"));
console.log(p.tag || "fallback");
console.log(p.tag === undefined);
console.log(p.tag ? "yes" : "no");
