// §27.2.4.{1,2,3,5} step 5.f — every element of a combinator's
// iterable goes through `promiseResolve(C, next)` BEFORE anything
// attaches to it, and for %Promise% that step adopts a thenable
// (§27.2.1.3 steps 2.f-2.l). tr skipped it: a non-promise element was
// taken for an already-fulfilled value and handed on as ITSELF.
//
// Two separate lanes had to learn this, and which one a program takes
// is decided by the STATIC element type, which is why a probe written
// one way sees only half the bug:
//   [{ then() {} }]            -> Array<Obj>  -> the iterator lane
//   [thenableTypedAsAny]       -> Array<Any>  -> the sync any lane
//   [thenable, "B"]            -> Array<Any>  -> ditto
const target = { tag: "V" };
const mk = () => ({ then(res: any) { res(target); } });

Promise.race([mk()]).then((v: any) => console.log("race", v === target));
Promise.all([mk()]).then((v: any) => console.log("all", v[0] === target));
Promise.any([mk()]).then((v: any) => console.log("any", v === target));
Promise.allSettled([mk()]).then(
  (r: any) => console.log("allSettled", r[0].status, r[0].value === target),
);

// A thenable that REJECTS must reject, not fulfill. Before this it
// fulfilled with the thenable object, which turns a failure into a
// success — the worst direction.
Promise.all([{ then(_res: any, rej: any) { rej(new Error("no")); } }]).then(
  () => console.log("reject", "WRONG: fulfilled"),
  (e: any) => console.log("reject", (e as Error).message),
);

// Mixed with plain values and real promises: the plain ones still
// win the race on their own tick, and a thenable no longer does.
Promise.race([mk(), "B"]).then((v: any) => console.log("mixed-race", v));
Promise.all([1, Promise.resolve(2), { then(r: any) { r(3); } }]).then(
  (v: any) => console.log("mixed-all", v.join(",")),
);

// The any-typed spelling of the same shape — the other lane.
const asAny: any = { then(res: any) { res(target); } };
Promise.race([asAny]).then((v: any) => console.log("any-lane", v === target));

// Plain values only: unchanged, and still no allocation road needed.
Promise.all([1, 2, 3]).then((v: any) => console.log("plain", v.join(",")));

// A real promise element still passes straight through.
Promise.race([Promise.resolve(target)]).then(
  (v: any) => console.log("native", v === target),
);
