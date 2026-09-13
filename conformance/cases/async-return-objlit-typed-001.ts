// An async function that returns an object LITERAL read its fields
// back wrong under a typed `await` once any generator class existed
// in the program (`4638707616191610880` for a stored 100 — the f64
// bits read as i64). Same defect as gen-async-step-typed-read-001,
// outside generators: the async rewrite turns `return {…}` into
// `return Promise.resolve({…})`, and a call argument takes no
// declared-layout pin, so the literal adsorbed onto the first
// same-shaped layout registered (the sync generator's `{value: any,
// done}` step) and boxed its number. A named alias (`type Step = …`)
// happened to read right; the inline spelling and a class method did
// not. The rewrite now binds the literal to a `const` declared as the
// inner type before resolving it. An async ARROW value (`const h =
// async (n) => …`) is not here: its awaited result takes no width
// injection at all (scalar `Promise<number>` included), a separate
// defect in the fn-value promise lane.
type Step = { value: number; done: boolean };
function* g1() {
  yield 1;
}
async function f(n: number): Promise<{ value: number; done: boolean }> {
  return { value: n * 100, done: false };
}
async function fa(n: number): Promise<Step> {
  return { value: n * 100, done: false };
}
class C {
  async m(n: number): Promise<{ value: number; done: boolean }> {
    return { value: n * 2.5, done: true };
  }
}
(async () => {
  const s = await f(1);
  console.log("fn", s.value, s.done);
  const sa = await fa(2);
  console.log("alias", sa.value, sa.done);
  const t = await new C().m(3);
  console.log("method", t.value, t.done);
  for (const x of g1()) console.log("sync", x);
})();
