// exit-code: 13
// A top-level await that nothing can settle any more, inside a
// `for…of` over an iterator with a `return()` and inside a `try` with a
// `finally`: the module body never goes on (§16.2.1.5.3), so neither
// the `finally` body nor IteratorClose runs (no completion ever leaves
// the loop). The program ends with the job queue empty and exits 13
// with a warning on stderr, as Node does for an unsettled top-level
// await.
//
// bun spins on the pending await, so the expected output is pinned
// beside this file.
class Steps {
  i = 0;
  [Symbol.iterator]() {
    return this;
  }
  next(): { value: number; done: boolean } {
    this.i++;
    return { value: this.i, done: this.i > 3 };
  }
  return(): { value: number; done: boolean } {
    console.log("return called");
    return { value: 0, done: true };
  }
}
const never: Promise<number> = new Promise(() => {});
try {
  for (const v of new Steps()) {
    console.log("step", v, await Promise.resolve(v * 2));
    if (v === 2) {
      const x = await never;
      console.log("after never", x);
    }
  }
} finally {
  console.log("finally ran");
}
console.log("unreachable");
