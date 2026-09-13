// 642-05 — an async VALUE nested inside another async body (or a
// generator), capturing that body's local by reference. Two layers
// to get right: the wrapper closure's minted self-name binds inside
// its own body, never in the enclosers' capture lists (n1 / n3, where
// the encloser keeps the synchronous lowering because its await sits
// inside a call); and when the encloser IS moved, its locals become
// iterator fields, so the nested generator reads them through a
// frame handle — the static link (n4 / n6 / n7 / n8).
// --- n1: async IIFE encloser, await inside a call (not moved)
(async () => {
  let order = "";
  const a = (async () => { await Promise.resolve(1); order += "a"; return 1; })();
  order += "s";
  console.log("n1", order, await a, order);
})();
// --- n3: async declaration encloser, same shape
async function n3() {
  let order = "";
  const a = (async () => { await Promise.resolve(1); order += "a"; return 3; })();
  order += "s";
  console.log("n3", order, await a, order);
}
n3();
// --- n4: encloser moved (statement-level await); nested writes its local
async function n4() {
  let order = "";
  const a = (async () => { await Promise.resolve(1); order += "a"; return 3; })();
  order += "s";
  const v = await a;
  console.log("n4", order, v, order);
}
n4();
// --- n5: encloser moved; nested reads a never-reassigned local (snapshot)
async function n5() {
  const tag = "t";
  let seen = "";
  const a = (async () => { await Promise.resolve(1); return tag + "!"; })();
  seen += "s";
  const v = await a;
  console.log("n5", seen, v);
}
n5();
// --- n6: the nested value is called twice; writes on both sides
async function n6() {
  let n = 0;
  const bump = async () => { await Promise.resolve(0); n = n + 1; return n; };
  const p1 = bump();
  n = 10;
  const r1 = await p1;
  const r2 = await bump();
  console.log("n6", r1, r2, n);
}
n6();
// --- n7: three levels — the deepest reads the outermost local
async function n7() {
  let log = "";
  const mid = async () => {
    await Promise.resolve(0);
    log += "m1";
    const deep = async () => { await Promise.resolve(0); log += "d"; return log.length; };
    const r = await deep();
    log += "m2";
    return r;
  };
  const p = mid();
  log += "s";
  const v = await p;
  console.log("n7", log, v);
}
n7();
// --- n8: the encloser is a plain generator
function* g8() {
  let n = 0;
  const bump = async () => { await Promise.resolve(0); n = n + 1; return n; };
  yield bump();
  n = 10;
  yield bump();
  yield n;
}
(async () => {
  const it = g8();
  const a = await it.next().value;
  const b = await it.next().value;
  console.log("n8", a, b, it.next().value);
})();
