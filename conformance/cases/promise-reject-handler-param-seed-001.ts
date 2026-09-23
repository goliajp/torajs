// Only the fulfilled handler receives the promise's inner value. The
// seed pass gave every handler argument of `.then` / `.catch` the
// inner annotation, so a rejection handler read its reason through the
// wrong width: a boxed string reaching an f64 parameter answers NaN,
// and `typeof e` folded to "number".
const p: Promise<number> = Promise.resolve(1);
p.then((v) => {
  if (v) throw "boom";
  return v + 1;
}).catch((e) => {
  console.log("catch", typeof e, e);
});

Promise.resolve(2)
  .then((v) => {
    throw { code: 7 };
  })
  .catch((e) => console.log("second", typeof e, JSON.stringify(e)));

Promise.reject("r").then(
  (v: number) => console.log("ok", v),
  (e) => console.log("onErr", typeof e, e),
);

// The fulfilled handler keeps the inner type, so arithmetic on it
// still lowers through the number lane.
Promise.resolve(41).then((v) => console.log("fulfilled", typeof v, v + 1));

// The reason reaching a handler must not be read through the inner
// width: `e + 1` on a string reason is concatenation, not NaN.
const r1: Promise<number> = Promise.reject("boom");
r1.then(
  (v) => console.log("never", v),
  (e) => console.log("onErr+", e + 1),
);
const r2: Promise<number> = Promise.reject("bam");
r2.catch((e) => console.log("catch+", e + 1, JSON.stringify(e)));
