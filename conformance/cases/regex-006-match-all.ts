// Phase 1c.3: s.matchAll(re) — Array<Array<Str>> stand-in for the
// JS iterator protocol. for-of and direct indexing work; bun's
// iterator semantics are deferred to Phase 1c.4+ when iterator
// protocol lands at the surface.

const all = "x12y34z56".matchAll(/(\w)(\d+)/g);
for (const m of all) {
  console.log(m[0], m[1], m[2]);
}

let n = 0;
for (const m of "abc".matchAll(/\d+/g)) {
  n = n + 1;
}
console.log("count:", n);

// Multi-capture matchAll
for (const m of "a1 b2 c3".matchAll(/(\w)(\d)/g)) {
  console.log(m[0], "->", m[1], m[2]);
}
