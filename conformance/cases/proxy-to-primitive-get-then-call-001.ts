// §7.1.1.1 step 5 over a Proxy: Get(O, name) is the `get` trap, then
// IsCallable — a trap answering a non-callable for `toString` moves the
// walk on to `valueOf` instead of throwing, and each name is read once.
// (Symbol-keyed reads are left out of the log: `@@toPrimitive` /
// `@@toStringTag` do not reach the trap yet.)
const log: string[] = [];
const p: any = new Proxy({}, {
  get(t, k) {
    if (typeof k === "string") log.push(k);
    if (k === "valueOf") return () => "V";
    return undefined;
  },
});
console.log(String(p), log.join());
log.length = 0;
console.log(p + "", log.join());
const p2: any = new Proxy({ toString() { return "T"; } }, {});
console.log(String(p2), `${p2}`);
const p3: any = new Proxy({}, { get() { return 5; } });
try {
  String(p3);
} catch (e) {
  console.log((e as any).constructor.name);
}
console.log(String(new Proxy({}, {}) as any), `${new Proxy([1, 2], {}) as any}`);
console.log((new Proxy({ valueOf() { return 1; } }, {}) as any) + 1);
const ct: string[] = [];
const p4: any = new Proxy({}, {
  get(t, k, r) {
    if (typeof k === "string") ct.push(k);
    return Reflect.get(t, k, r);
  },
});
console.log(String(p4), ct.join());
