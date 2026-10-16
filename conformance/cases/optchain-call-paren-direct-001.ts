// `(o?.m)(args)`: the parentheses end the optional chain (§13.3.9), so
// a nullish `o` makes the callee `undefined`; the arguments still
// evaluate and the call throws a TypeError. A present `o` stays the
// receiver, as in `o.m(args)`.

type T = { k: number; m(x: number): number };
function make(n: number): T | undefined {
  return n > 0
    ? {
        k: 10,
        m(x: number) {
          return this.k + x;
        },
      }
    : undefined;
}
const log: string[] = [];
function arg(v: number): number {
  log.push("arg" + v);
  return v;
}

function typed(o: T | undefined, v: number): string {
  try {
    return String((o?.m)(arg(v)));
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}
function viaAny(o: any, v: number): number {
  return (o?.m)(arg(v));
}

const n = process.argv.length > 99 ? 0 : 1;
console.log(typed(make(n), 1), typed(make(n - 1), 2));
console.log(viaAny(make(n), 3));
for (const o of [null, undefined]) {
  try {
    console.log(viaAny(o, 4));
  } catch (e) {
    console.log("caught", e instanceof TypeError);
  }
}
const short: any = null;
console.log(short?.m(arg(5)));
console.log(log.join(","));
