// RFC 20260915 registered item 1 — a for-of / for-await whose head is a
// destructuring PATTERN runs inside the async machine like any other
// loop: an object rest lifted to a field types `any` (objlit_ann's
// spread arm), so the gate's `__forof_destr_` refusal and the forof
// rewrite's exception are gone. Every shape below suspends mid-loop
// (`sync` prints first); the destructured binds — rest objects, array
// rest / elision, nested patterns, defaults evaluated per iteration,
// computed keys beside a rest, a rest object that stays extensible —
// read the same values bun prints.
async function objRest() {
  const out: any[] = [];
  for (const { a, ...r } of [{ a: 1, b: 2, c: 3 }, { a: 4, d: 5 }]) { await null; out.push(a, r); }
  for await (const { x: { y = 5 } = {}, ...z } of [{ x: { y: 1 }, k: 2 }, { m: 3 }]) { out.push(y, z); }
  console.log(JSON.stringify(out));
}
async function aryRest() {
  const out: any[] = [];
  for await (const [, ...r] of [[1, 2, 3], [4]]) { out.push(r); }
  for await (const [a, , c = 9] of [[1, 2, 3], [4]]) { await null; out.push(a, c); }
  for await (const [a = 10, { b, c = 7 } = { b: 1 }] of [[undefined, { b: 2 }], [3]]) { out.push(a, b, c); }
  console.log(JSON.stringify(out));
}
async function* srcGen() { yield [1, 2, 3]; yield [4, 5]; yield [6]; }
async function asyncSource() {
  const out: any[] = [];
  for await (const [h, ...t] of srcGen()) { out.push(h, t); if (h === 4) break; }
  console.log(JSON.stringify(out));
}
async function* genRest() {
  for await (const { a, ...r } of [{ a: 1, b: 2 }, { a: 3, c: 4 }]) { yield [a, r]; }
  for await (const [, ...t] of [[1, 2], [3]]) { console.log("t", JSON.stringify(t)); }
}
async function defaultsAndComputed() {
  const k = "b";
  const out: any[] = [];
  for await (const { a, [k]: kk, ...r } of [{ a: 1, b: 2, c: 3 }]) { out.push(a, kk, r); }
  for await (const { length } of ["abc", "de"]) { await null; out.push(length); }
  let n = 0;
  for await (const [x = n++, y = n++] of [[], [1]]) { out.push(x, y); }
  console.log(JSON.stringify(out));
}
async function errorsAndExtensible() {
  const out: any[] = [];
  try {
    for await (const [a] of [null as any]) { out.push(a); }
  } catch (e: any) { out.push(e.constructor.name); }
  try {
    for await (const { a } of [undefined as any]) { out.push(a); }
  } catch (e: any) { out.push(e.constructor.name); }
  for await (const { a, ...r } of [{ a: 1, b: 2 }]) { await null; r.z = 9; out.push(r.z, r.b, Object.keys(r).length); }
  console.log(JSON.stringify(out));
}
function* closing() { try { yield [1, 2]; yield [3, 4]; } finally { console.log("closed"); } }
async function varHead() {
  for (const [a, b] of closing()) { await null; console.log(a, b); if (a === 1) break; }
  for await (var [p, q] of [[7, 8]]) { console.log(p, q); }
  console.log(p, q);
}
(async () => {
  await objRest();
  await aryRest();
  await asyncSource();
  for await (const v of genRest()) console.log(JSON.stringify(v));
  await defaultsAndComputed();
  await errorsAndExtensible();
  await varHead();
  console.log("done");
})();
console.log("sync");
