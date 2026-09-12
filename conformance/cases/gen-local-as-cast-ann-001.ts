// A generator local initialized with an `as` cast took the `number`
// fallback in the state-machine field sniff — nothing read through the
// cast — so `const a = (Promise as any).all; a.call(Promise, …)` failed
// check with "no member `.call` on type Number". The cast's annotation
// IS the local's type. Reproduced without any async rewrite; the same
// shape is how `promise-combinator-recv-first-001` reads once an async
// body becomes a generator (RFC 20260805 blade 1).
function* g(): any {
  const a = (Promise as any).all;
  const r = yield a.call(Promise, [1, 2, 3]);
  console.log("all.call", r[0], r[1], r[2]);
  const label = ("all" as any) as string;
  console.log("cast-string", typeof label, label.length);
  const race = (Promise as any).race;
  const w = yield race.call(Promise, [Promise.resolve(9)]);
  console.log("race.call", w);
}
const it = g();
it.next().value.then((v: any) => {
  const step = it.next(v);
  step.value.then((x: any) => {
    it.next(x);
  });
});
