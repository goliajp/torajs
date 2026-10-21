// The un-annotated parameter of the handler after `.then(onOk, onErr)`
// was seeded from onOk's return alone, so a value settled by onErr
// reached it in onOk's form: a string as NaN, an undefined as NaN.
// After `.catch(onErr)` it was seeded from onErr's return alone, so a
// fulfilled value passing through reached it in onErr's form and
// crashed. §27.2.5.4 settles the derived promise from whichever handler
// runs (a fulfilled source passes `.catch` unchanged), so the parameter
// takes the union of the two legs' values.
const ok: Promise<number> = Promise.resolve(1);
const bad: Promise<number> = Promise.reject("boom");

ok.then(
  (x) => x * 2,
  (e) => "rejected",
).then((v) => console.log("num leg:", v, typeof v));
bad.then(
  (x) => x * 2,
  (e) => "rejected",
).then((v) => console.log("str leg:", v, typeof v));

bad.then(
  (x) => x + 1,
  (e) => undefined,
).then((v) => console.log("undefined leg:", v, typeof v, v === undefined ? 0 : v + 1));
ok.then(
  (x) => x + 1,
  (e) => undefined,
).then((v) => console.log("value leg:", v, v === undefined ? 0 : v + 1));

bad.then(
  (x) => x,
  (e) => {
    console.log("void leg ran:", e);
  },
).then((v) => console.log("void leg:", v));

ok.then(
  (x) => x + 0.5,
  (e) => 7,
).then((v) => console.log("same type:", v * 2));

bad.then(
  (x) => "s" + x,
  (e) => Promise.resolve(0.25),
).then((v) => console.log("promise leg:", v, typeof v));

ok.then(
  (x) => x * 3,
  (e) => {
    throw new Error("never runs");
  },
).then((v) => console.log("throw leg:", v + 1));

ok.catch((e) => "caught").then((v) => console.log("catch pass:", v, typeof v));
bad.catch((e) => "caught").then((v) => console.log("catch run:", v, typeof v));
bad.catch((e) => 42).then((v) => console.log("catch same:", v + 1));

Promise.resolve("a")
  .then(
    (s) => s.length,
    (e) => "none",
  )
  .then((v) => (typeof v === "number" ? v * 10 : v))
  .then((w) => console.log("chain:", w));

async function inBody(flag: boolean, src: Promise<number>): Promise<void> {
  src
    .then(
      (x) => x + 1,
      (e) => "body " + e,
    )
    .then((v) => console.log("body then:", flag, v, typeof v));
  const r = await src.then(
    (x) => x * 2,
    (e) => "awaited " + e,
  );
  console.log("body await:", flag, r);
  const c = await src.catch((e) => -1);
  console.log("body catch:", flag, c + 1);
}

const five: Promise<number> = Promise.resolve(5);
const no: Promise<number> = Promise.reject("no");
await inBody(true, five);
await inBody(false, no);
const top = await bad.then(
  (x) => x,
  (e) => "top " + e,
);
console.log("top:", top);
