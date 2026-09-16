// A `this`-reading STATIC method's receiver channel lives on its
// `__smany_` twin — the mono has none at all. Its value was a shell
// that pins the receiver to `undefined`, so once `.bind` stopped being
// swallowed by the static synth lane and started riding the runtime
// kernel, the bound this had nowhere to go: `C.method.bind(C)` lost it.
// `.call` / `.apply` were devirtualized to the twin already; `.bind`
// takes the same road now, and a `__this`-taking declaration's shell is
// the receiver-first one.

class C {
  static tag = "C";
  static readIt(this: any, n: number): string {
    return (this as any).tag + ":" + n;
  }
}
const bound: any = (C as any).readIt.bind(C);
console.log(bound(1));
console.log((C as any).readIt.call(C, 2));
console.log((C as any).readIt.apply(C, [3]));

const rebound: any = bound.bind({ tag: "other" });
console.log("rebound keeps the first this:", rebound(4));

const other = { tag: "O" };
console.log((C as any).readIt.bind(other)(5));

// partial arguments concatenate ahead of the call's own
const p: any = (C as any).readIt.bind(C, 9);
console.log(p());
