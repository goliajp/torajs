// An object literal's `async m() {}` method is an async function value: it closes
// over the enclosing scope (a counter in the method that built it), takes the
// object as `this` when called as a method, and awaits like any async function.
// The computed-key spelling and the named spelling are the same production.
const o: any = {
  [Symbol.asyncIterator]() {
    let i = 0;
    return {
      async next() {
        if (i >= 3) {
          return { done: true, value: undefined };
        }
        i++;
        await null;
        return { done: false, value: i * 10 };
      },
    };
  },
};
function make(tag: string): any {
  let n = 0;
  return {
    async bump() {
      n++;
      return tag + n;
    },
    async peek() {
      return this.label;
    },
    label: tag + "!",
  };
}
async function main(): Promise<void> {
  const out: any[] = [];
  for await (const v of o) {
    out.push(v);
  }
  console.log(out.join(","));
  console.log((await Array.fromAsync(o)).join(","));
  const a = make("a");
  const b = make("b");
  console.log(await a.bump(), await a.bump(), await b.bump());
  console.log(await a.peek(), await b.peek());
  console.log("done");
}
main();
