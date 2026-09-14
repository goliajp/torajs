// RFC 20260915 registered item 2 — the temporal dead zone of a machine
// body's own `let` / `const` (§8.1.1.1). Lifted to fields, these
// bindings used to answer their zero when read before the declaration
// ran; now a read the statement order settles raises in place, and a
// closure written ahead of the declaration reads through the binding's
// initialised-flag. Every shape suspends (`sync` prints first) so the
// body is the async machine, not the synchronous lowering.
async function listOrder() {
  await null;
  let x = 1;
  try { const y = y + 1; } catch (e: any) { console.log("a1", e.constructor.name); }
  { try { console.log(z); } catch (e: any) { console.log("a2", e.constructor.name); } await null; let z = 2; console.log("a3", z); }
  for (let i = 0; i < 2; i++) {
    if (i) { try { console.log(w); } catch (e: any) { console.log("a4", e.constructor.name); } }
    let w = i; await null; console.log("a5", w);
  }
  try { q = 5; } catch (e: any) { console.log("a6", e.constructor.name); }
  let q = 0;
  console.log("a7", x, q);
}
async function closures() {
  function f() { return x + 1; }
  function s() { x = 2; }
  const g = () => x;
  try { f(); } catch (e: any) { console.log("b1", e.constructor.name); }
  try { s(); } catch (e: any) { console.log("b2", e.constructor.name); }
  try { g(); } catch (e: any) { console.log("b3", e.constructor.name); }
  await null;
  let x = 1;
  s();
  console.log("b4", f(), g(), x);
  const h = () => k;
  const k = 3;
  console.log("b5", h());
  for (let i = 0; i < 2; i++) {
    const r = () => m;
    if (i) { try { r(); } catch (e: any) { console.log("b6", e.constructor.name); } }
    let m = i * 10; await null;
    console.log("b7", r());
  }
}
async function awaitUsing() {
  function f() { return x + 1; }
  try { f(); } catch (e: any) { console.log("c1", e.constructor.name); }
  await using x = null;
  console.log("c2", f());
  try { console.log(y); } catch (e: any) { console.log("c3", e.constructor.name); }
  await using y = null;
  let z = { async [Symbol.asyncDispose]() { } };
  try { for (await using z of [z]) {} } catch (e: any) { console.log("c4", e.constructor.name); }
}
async function* gen() {
  const f = () => v;
  try { f(); } catch (e: any) { yield "d1 " + e.constructor.name; }
  const v = 7;
  yield "d2 " + f();
}
(async () => {
  await listOrder();
  await closures();
  await awaitUsing();
  for await (const s of gen()) console.log(s);
  console.log("done");
})();
console.log("sync");
