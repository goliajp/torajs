// §27.2.1.3.2 — a reaction handler returning a promise makes the
// derived promise ADOPT it: the next handler sees the unwrapped value,
// not a promise object. Covers a settled inner, a still-pending inner,
// a rejecting inner, and the .catch / 2-arg .then legs.

// settled inner
Promise.resolve(1)
  .then(() => Promise.resolve(2))
  .then((v) => console.log("A", v));

// pending inner — the outer waits for it, it does not reject
function later(): Promise<number> {
  return new Promise((res) => {
    res(5);
  });
}
Promise.resolve(1)
  .then(() => later())
  .then((v) => console.log("B", v));

// inner rejects -> the outer rejects with its reason
Promise.resolve(1)
  .then(() => Promise.reject(new Error("c1")))
  .catch((e: any) => console.log("C", e.message));

// .catch handler returning a promise recovers through it
Promise.reject(new Error("d1"))
  .catch(() => Promise.resolve(9))
  .then((v) => console.log("D", v));

// 2-arg .then, fulfilled leg returns a promise
Promise.resolve(1).then(
  () => Promise.resolve(11),
  () => Promise.resolve(-1),
).then((v) => console.log("E", v));

// 2-arg .then, rejection leg returns a promise
Promise.reject(new Error("f1")).then(
  () => Promise.resolve(-1),
  () => Promise.resolve(12),
).then((v) => console.log("F", v));

// an async handler's promise adopts the same way
async function twice(x: number) {
  return x * 2;
}
Promise.resolve(7)
  .then((v) => twice(v))
  .then((v) => console.log("G", v));

// a non-promise return is still fulfilled verbatim
Promise.resolve(3)
  .then((v) => v + 1)
  .then((v) => console.log("H", v));
