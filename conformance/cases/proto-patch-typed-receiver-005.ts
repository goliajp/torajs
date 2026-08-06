// RFC 20260806 — the Promise family stands down too.
//
// A `Promise<T>`-typed receiver reached its `then` / `catch` /
// `finally` kernels directly, so a patch on `Promise.prototype` was
// invisible to it while an `any`-typed receiver saw it. Three probes,
// three misses, zero exceptions.

const p: Promise<number> = Promise.resolve(1);

// Registered BEFORE any patch: this one keeps the kernel's behaviour,
// and its callback runs on the microtask queue — after all the
// synchronous logging below. The bitmap is read when the call runs,
// so sequencing is what decides, not compilation.
p.then((v: any) => {
  console.log("kernel then saw", v);
});

(Promise.prototype as any).then = function () {
  return "PATCHED-then";
};
(Promise.prototype as any).catch = function () {
  return "PATCHED-catch";
};
(Promise.prototype as any).finally = function () {
  return "PATCHED-finally";
};

const a: any = p.then((v: any) => v);
const b: any = p.catch((v: any) => v);
const c: any = p.finally(() => {});
console.log(a, b, c);

// An `any`-typed receiver has always seen the patch — the two lanes
// have to agree now that the typed one consults the same bitmap.
const q: any = Promise.resolve(2);
console.log(q.then((v: any) => v), q.catch((v: any) => v));
