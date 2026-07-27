// RFC 20260727-dstr-decl-shape 刀 B — for-of declaration-head
// patterns through the recursive PatShape machine. The old scan
// accepted only flat name lists; defaults / elisions / rest / nested
// patterns / renames all bailed to C-style and died on `expected ;`.
//
// Recorded boundaries this fixture dodges (pre-existing element-lane
// holes, minimal repros in plan-state L3b, NOT pattern bugs):
// - hole X: a heterogeneous Array<Array> source
//   (`[[1], [undefined, 2], []]`) reads garbage bits per element
//   (`5e-323 NaN`) — the S2.24 assignment head shows the identical
//   wrong bits, statement position is clean.
// - hole Y: a mixed struct array's undefined-valued field reads back
//   `0` (`for (const o of [{r:2},{r:undefined}]) o.r` — no pattern
//   involved). Sources here keep every field populated.

// flat names (the previously-working face — must stay working)
for (const [a, b] of [[1, 2], [3, 4]]) {
  console.log(a, b);
}

// defaults fire on an in-tuple undefined (single-tuple source stays
// off hole X)
for (const [x = 10, y = 20] of [[undefined, 2]]) {
  console.log(x, y);
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
// case needs hole Y closed first
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
