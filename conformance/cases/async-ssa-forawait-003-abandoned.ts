// Async functions left waiting inside a `for await` (an array element and
// an async iterator's step that never settle) are dropped without running
// their `finally` or the iterator's `return()`, and a loop of short
// `for await` runs keeps its memory flat.
const never = new Promise<number>(() => {});
async function stuckArr(tag: string) {
  const held = { tag };
  try {
    for await (const x of [1, never]) console.log(held.tag, "step", x);
  } finally {
    console.log("finally must not run", held.tag);
  }
}
async function stuckAny(src: any) {
  for await (const x of src) console.log("any step", x);
}
async function churn(n: number) {
  let s = 0;
  for (let i = 0; i < n; i++) {
    for await (const x of [i, Promise.resolve(1)]) s += x;
  }
  return s;
}
stuckArr("a");
stuckAny([2, never]);
const anyIt: any = {
  [Symbol.asyncIterator]() {
    return { next() { return never.then(() => ({ done: true })); }, return() { console.log("no"); return {}; } };
  },
};
stuckAny(anyIt);
churn(100000).then(s => console.log("churn", s));
