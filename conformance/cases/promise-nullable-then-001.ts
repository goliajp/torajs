// `Promise<T | null>.then(cb)` was refused outright — "no member
// `.then` on type Promise(Nullable(String))". Not a wrong answer: no
// answer at all, on a shape the runtime already carried correctly.
// Every other inner shape got a lane in the chain as it was unlocked;
// the nullable one never did.
const s: string | null = null;
Promise.resolve(s).then((v) => {
  console.log("str null:", v);
});

const n: number | null = null;
Promise.resolve(n).then((v) => {
  console.log("num null:", v);
});

const b: boolean | null = null;
Promise.resolve(b).then((v) => {
  console.log("bool null:", v);
});

// A nullable holding a value reads as that value.
const hit: string | null = "hi";
Promise.resolve(hit).then((v) => {
  console.log("str value:", v);
});

// The cb's return type drives the chain's, as in the sibling lanes.
const m: number | null = null;
Promise.resolve(m)
  .then((v) => (v === null ? "was null" : "was " + v))
  .then((w) => {
    console.log("chained:", w);
  });

// An `any` parameter is admitted the same way the sibling lanes
// admit it.
const a2: string | null = null;
Promise.resolve(a2).then((v: any) => {
  console.log("any param:", v);
});

// `.catch` shares the shape; a fulfilled source passes through it.
const c: string | null = null;
Promise.resolve(c)
  .catch((e) => e)
  .then((v) => {
    console.log("through catch:", v);
  });
