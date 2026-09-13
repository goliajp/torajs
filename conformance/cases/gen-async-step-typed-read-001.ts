// `await it.next()` on an async generator read its step's `value` back
// as a wrong number as soon as a SECOND generator class existed in the
// program (`a 128 false` for a stored 100; two async generators: the
// stored f64 read back as its raw i64 bits). Two halves, both in the
// async lane only — the sync lane's `return {value, done}` sits in a
// fn declared to return `__step_<gen>` and takes the ret-lowering's
// declared-layout pin:
//  - writer: the async rewrite hands the step literal to
//    `Promise.resolve(...)` as a call argument, so no site pinned the
//    declared `__step_<gen>` layout and the un-hinted literal took the
//    first same-named layout registered (a sync generator's `{value:
//    any, done}`), boxing its value into a slot every reader loaded as
//    f64. The literal now binds to a `const` declared as the inner
//    type first (`desugar_async::rewrite_returns_for_async`).
//  - reader: the await's struct widening asked num_width at the
//    PROMISE key instead of its value point, so a single-class
//    program's direct `__cm_` call read the parse default (I64) while
//    the writer had widened to F64 (`widen_promise_inner_ty`).
// `console.log(s)` and an `any`-typed read of the same pointer, going
// by the object's class tag, always answered right — only the typed
// read was wrong. Every await here is in a function the async rewrite
// does not move (an assignment await), so it takes the typed read.
function* sync1() {
  yield 1;
}
const ag = async function* (n: number): AsyncGenerator<number> {
  yield n * 100;
  yield n * 100 + 1;
};
async function* ag2(s: string): AsyncGenerator<string> {
  yield s + "!";
}
async function* ag3(n: number): AsyncGenerator<number> {
  yield n * 3;
}
(async () => {
  const it = ag(7);
  let step = await it.next();
  while (!step.done) {
    console.log("ag", step.value, step.done, step.value + 1);
    step = await it.next();
  }
  console.log("ag-end", step.value, step.done);
  const jt = ag2("hi");
  let js = await jt.next();
  while (!js.done) {
    console.log("ag2", js.value, js.done);
    js = await jt.next();
  }
  const kt = ag3(5);
  let ks = await kt.next();
  console.log("ag3", ks.value, ks.done);
  ks = await kt.next();
  console.log("ag3-end", ks.value, ks.done);
  const s1 = sync1();
  console.log("sync", s1.next().value, s1.next().done);
})();
