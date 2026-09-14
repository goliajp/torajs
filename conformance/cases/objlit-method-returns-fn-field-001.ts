// A method shorthand / arrow whose return is an object literal carrying a declared
// function as a field value: the field holds the FUNCTION (a closure over the
// declaration), not what a call of it would answer — `o.m().next` is callable.
// The same with an `async` function and with an async method shorthand.
function sf(x: number): number {
  return x + 1;
}
async function af(): Promise<number> {
  return 7;
}
const o1: any = {
  m() {
    return { next: sf };
  },
};
const o2: any = { m: () => ({ next: af, tag: "t" }) };
// (the async method reads a top-level counter: an object literal's async
// method is a hoisted declaration today and captures nothing — registered)
let gi = 0;
const o3: any = {
  [Symbol.asyncIterator]() {
    gi = 0;
    return {
      async next() {
        if (gi >= 2) {
          return { done: true, value: undefined };
        }
        gi++;
        return { done: false, value: gi * 10 };
      },
    };
  },
};
async function main(): Promise<void> {
  console.log(typeof o1.m().next, o1.m().next(1));
  console.log(typeof o2.m().next, o2.m().tag, await o2.m().next());
  const out: any[] = [];
  for await (const v of o3) {
    out.push(v);
  }
  console.log(out.join(","));
  console.log((await Array.fromAsync(o3)).join(","));
  console.log("done");
}
main();
