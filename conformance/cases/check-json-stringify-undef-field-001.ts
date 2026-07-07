// Chunk 658 — an undefined Str field skips its KEY per §25.5.2.4
// step 8.b (SerializeJSONProperty returns undefined → no key, no
// stray comma). Covers both emit lanes: the jsb builder fast path
// (primitive-only layouts, runtime pending_sep protocol) and the
// str_concat slow lane (mixed layouts, json_obj_sep + sentinel
// branch per Str field).

const m = /a(b)?/.exec("a");
if (m !== null) {
  const u = m[1];

  // Fast path (Str/I64/Bool only).
  console.log(JSON.stringify({ a: u, b: "y" }));
  console.log(JSON.stringify({ a: u }));
  console.log(JSON.stringify({ b: "y", a: u }));
  console.log(JSON.stringify({ n: 1, a: u, ok: true }));

  // Slow lane (F64 forces the concat chain).
  console.log(JSON.stringify({ a: u, b: 1.5, c: u, d: "z" }));
  console.log(JSON.stringify({ x: 1.5, a: u }));

  // Hit lane regression — all keys present.
  console.log(JSON.stringify({ a: m[0], n: 2 }));
  console.log(JSON.stringify({ a: m[0], b: 0.5 }));
}
