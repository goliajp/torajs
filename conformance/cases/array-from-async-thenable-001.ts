// proposal-array-from-async §2.1.1 — a thenable element off a sync iterable or an
// array-like is awaited once (one `then` read, one call); a rejecting thenable off a
// sync iterator closes the iterator; a `then` that throws rejects the whole result.
const log: string[] = [];
function mk(tag: string, v: any): any {
  const t: any = {};
  Object.defineProperty(t, "then", {
    get() {
      log.push("get " + tag + ".then");
      return function (res: any, rej: any) {
        log.push("call " + tag + ".then");
        res(v);
      };
    },
    configurable: true,
  });
  return t;
}
async function main(): Promise<void> {
  const r1 = await Array.fromAsync([mk("a", 1), mk("b", 2)]);
  console.log(r1.join(","), log.join("|"));
  log.length = 0;
  const r2 = await Array.fromAsync({ length: 2, 0: mk("c", 3), 1: Promise.resolve(4) });
  console.log(r2.join(","), log.join("|"));
  let closed = 0;
  const it: any = {
    [Symbol.iterator]() {
      return {
        next() {
          return {
            value: {
              then(res: any, rej: any) {
                rej(new Error("no"));
              },
            },
            done: false,
          };
        },
        return() {
          closed++;
          return {};
        },
      };
    },
  };
  try {
    await Array.fromAsync(it);
    console.log("BAD: fulfilled");
  } catch (e: any) {
    console.log("rej", e.message, closed);
  }
  const bad: any = {
    length: 1,
    0: {
      then() {
        throw new RangeError("thrown");
      },
    },
  };
  try {
    await Array.fromAsync(bad);
    console.log("BAD: fulfilled");
  } catch (e: any) {
    console.log("thrown", e instanceof RangeError);
  }
  console.log("done");
}
main();
