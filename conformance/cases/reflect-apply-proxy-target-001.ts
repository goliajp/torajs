// §28.1.1 step 1 is IsCallable(target). A Proxy is callable when its
// target is (§10.5.14 step 7) and runs its `apply` trap, so it never
// reaches the closure classifier this gate used — which knows closure
// cells only, and said "requires the first argument be a function"
// about a proxy over an ordinary function.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
function F(x: number): number {
  return x + 1;
}
const pf: any = new Proxy(F as any, {});
console.log("plain proxy   ", attempt(() => Reflect.apply(pf, null, [1])));
console.log("apply trap    ", attempt(() =>
  Reflect.apply(
    new Proxy(F as any, {
      apply(_t: any, _th: any, args: any) {
        return args[0] * 10;
      },
    }),
    null,
    [3],
  )));
console.log("non-callable  ", attempt(() => Reflect.apply(new Proxy({} as any, {}), null, [1])));
console.log("nullish list  ", attempt(() => (Reflect as any).apply(pf, null, null)));
console.log("plain fn still", attempt(() => Reflect.apply(F as any, null, [1])));
