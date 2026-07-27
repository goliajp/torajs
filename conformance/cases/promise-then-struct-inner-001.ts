// .then / .catch on Promise(Struct) — the async-generator method trio
// desugars to Promise.resolve({value, done}), so gen().next() answers
// Promise(Struct([("value", Any), ("done", Boolean)])). Covers: two-arg
// then with unannotated params, chained second hop (Void-return handler
// -> Promise(Undefined) receiver), the rejected path, .catch, a 0-arg
// handler, a typed struct param, and value propagation through the chain.
async function* g1() { yield 1; yield 2; }

g1().next().then(
  function (r) { console.log("p1 ok", r.value, r.done); },
  function (e) { console.log("p1 err", e); }
);

g1().next()
  .then((r) => { console.log("p2 first", r.done); })
  .then(() => console.log("p2 second"), (e) => console.log("p2 err", e));

function* thrower(): Generator<number> { throw new Error("boom"); }
async function* g2() { yield* thrower(); }
g2().next().then(
  function (r) { console.log("p3 ok", r); },
  function (e) { console.log("p3 rejected", (e as Error).message); }
);

g2().next().catch((e) => console.log("p4 caught", (e as Error).message));

g1().next().then(() => console.log("p5 zero-arg ok"));

async function* g4() { yield 7; }
g4().next().then((r: {value: any, done: boolean}) => {
  console.log("p6", r.value, r.done);
});

g4().next().then((r) => r.value).then((v) => console.log("p7", v));
