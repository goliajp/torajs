// proposal-array-from-async §2.1.1 step 3.h.iv.9 — mapfn's result is awaited once per
// element, interleaved with the call (call mapping k, get then k, call then k); the
// thisArg reaches mapfn; an async mapfn's promise is awaited; a non-callable mapfn
// rejects before the source is touched (step 3.b.i runs ahead of GetIterator).
const calls: string[] = [];
function mapping(v: any, k: any): any {
  calls.push("call mapping " + k);
  const t: any = {};
  Object.defineProperty(t, "then", {
    get() {
      calls.push("get then " + k);
      return (res: any) => {
        calls.push("call then " + k);
        res(v * 2);
      };
    },
  });
  return t;
}
async function main(): Promise<void> {
  const r = await Array.fromAsync([1, 2, 3], mapping);
  console.log(r.join(","));
  console.log(calls.join("|"));
  const r2 = await Array.fromAsync(
    [1, 2],
    function (this: any, v: any, k: any) {
      return this.tag + v + k;
    },
    { tag: "T" }
  );
  console.log(r2.join(","));
  const r3 = await Array.fromAsync([1, 2], async (v: any) => v + 10);
  console.log(r3.join(","));
  let touched = false;
  const src: any = {
    [Symbol.iterator]() {
      touched = true;
      return {
        next() {
          return { done: true };
        },
      };
    },
  };
  try {
    await Array.fromAsync(src, 5 as any);
    console.log("BAD: fulfilled");
  } catch (e) {
    console.log("noncallable", e instanceof TypeError, touched);
  }
  console.log("done");
}
main();
