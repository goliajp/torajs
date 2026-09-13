// An async function VALUE's awaited result took no width injection:
// `promise_obj_keys` keyed a call through an fn-valued name by its
// Anon origin, while num_width files that result under the value's
// `__ret` projection (glued onto the lifted closure's Ret at the
// binding's init). Nothing joined, so a fractional `Promise<number>`
// read back as its f64 bits (`4616752568008179712` for 4.5) and a
// struct payload read at the parse default while the closure had
// widened. A declared async FUNCTION (`async function g`) was always
// right — its call keys through `Ret(g)`. The call arm now projects
// the value's `__ret` the same way the analysis does.
type Step = { value: number; done: boolean };
function* g1() {
  yield 1;
}
const h = async (n: number): Promise<number> => n + 0.5;
const hs = async (n: number): Promise<{ v: number }> => {
  return { v: n * 2.5 };
};
const hf = async function (n: number): Promise<Step> {
  return { value: n / 4, done: false };
};
async function g(n: number): Promise<number> {
  return n + 0.25;
}
function viaCapture(base: number) {
  const k = async (x: number): Promise<number> => base + x / 2;
  return k(3);
}
(async () => {
  console.log("arrow", await h(4));
  const s = await hs(4);
  console.log("arrow-struct", s.v);
  const t = await hf(6);
  console.log("fnexpr-step", t.value, t.done);
  console.log("decl", await g(4));
  console.log("capture", await viaCapture(10));
  for (const x of g1()) console.log("sync", x);
})();
