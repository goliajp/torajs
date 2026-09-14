// A binding pattern's REST over an iterator source, inside a machine
// body: the group temp is lifted to a field and its store walks the
// iterator protocol (draining for the rest, §8.6.2) instead of holding
// the raw source — `for (const [...x] of [gen()])` used to fail on
// `.slice` of a generator object in every machine lane (a sync
// generator with a suspending body, an async function's awaiting
// for-of, a for-await). Iterator close on early return, an elision
// under a rest, and a throwing `next` read the same as bun.
function mk() { return (function* () { yield 1; yield 2; })(); }
function* sg() { for (const [...x] of [mk()]) { yield x.length; } }
console.log("v1", [...sg()]);
async function v2() { for (const [...x] of [mk()]) { await null; console.log("v2", x.length); } }
async function v3() { for await (const [...x] of [mk()]) { console.log("v3", x.length); } }
async function v4() { for await (const [a, b] of [mk()]) { console.log("v4", a, b); } }
async function v5() { for await (const [a, ...r] of [mk()]) { await null; console.log("v5", a, r); } }
const iter = (function* () { yield; yield; })();
async function v6() { for await (const [...x] of [iter]) { return; } }
let first = 0, second = 0;
function* g() { first += 1; yield; second += 1; }
async function v7() { for await (const [...[,]] of [g()]) { console.log("v7", first, second); } }
const bad = (function* () { throw new Error("boom"); })();
async function v8() { for await (const [, ...x] of [bad]) { return; } }
async function* ag() { for await (const [...x] of [mk()]) { yield x.length; } }
async function v9() { for await (const v of ag()) console.log("v9", v); }
async function v10() { for await (const [h, ...t] of ["xyz"]) { await null; console.log("v10", h, t); } }
(async () => {
  await v2(); await v3(); await v4(); await v5();
  await v6(); console.log("v6", iter.next().done);
  await v7();
  await v8().then(() => console.log("v8 resolved"), (e: any) => console.log("v8 rejected", e.message));
  await v9();
  await v10();
  console.log("done");
})();
console.log("sync");
