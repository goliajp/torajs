// RFC 20260727-dstr-decl-shape 刀 B — for-of declaration-head
// patterns through the recursive PatShape machine. The old scan
// accepted only flat name lists; defaults / elisions / rest / nested
// patterns / renames all bailed to C-style and died on `expected ;`.
//
// Holes X and Y (the garbage-bits / forced-anchor-layout faces) are
// FIXED — see arr-hetero-struct-001, which asserts the previously
// dodged shapes, including the heterogeneous-source defaults below.
// Recorded residual this fixture still dodges: a pure-undefined
// VALUED struct field reads back null instead of undefined (RFC
// 20260710 C2b family, kind-less Ptr slot) — so the object-pattern
// default here keeps its fields populated.

// flat names (the previously-working face — must stay working)
for (const [a, b] of [[1, 2], [3, 4]]) {
  console.log(a, b);
}

// defaults fire on an in-tuple undefined
for (const [x = 10, y = 20] of [[undefined, 2]]) {
  console.log(x, y);
}

// defaults over a heterogeneous source (the hole-X shape, now fixed)
for (const [hx = 10, hy = 20] of [[1], [undefined, 2], []]) {
  console.log(hx, hy);
}

// defaults NOT fired when values are present
for (const [x2 = 10, y2 = 20] of [[1, 2], [3, 4]]) {
  console.log(x2, y2);
}

// elision + rest
for (const [, second, ...tail] of [[1, 2, 3, 4]]) {
  console.log(second, tail.length, tail[0]);
}

// nested array element
for (const [[m, n], k] of [[[1, 2], 3]]) {
  console.log(m, n, k);
}

// object pattern with rename and (not-fired) default — the fired
// case needs the null/undefined Ptr-slot residual closed first
for (const { p: q, r = 7 } of [{ p: 1, r: 2 }, { p: 3, r: 9 }]) {
  console.log(q, r);
}

// obj-in-obj nested
for (const { u: { v } } of [{ u: { v: 42 } }]) {
  console.log(v);
}

// let-form pattern head
for (let [h1, h2] of [[5, 6]]) {
  console.log(h1, h2);
}
