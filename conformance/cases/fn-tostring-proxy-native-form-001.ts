// §20.2.3.5 step 4 — `Function.prototype.toString` on a Proxy. A
// proxy has no [[SourceText]] slot (step 2) and is not a built-in
// function object (step 3), so a callable one lands on step 4: an
// implementation-defined string with the syntax of a NativeFunction.
// JSC (so bun) spells it `function ProxyObject() { [native code] }`.
//
// tr answered the TARGET's source, because that is what a trap-less
// proxy does for every other method — forward the dispatch. toString
// is the one method whose answer turns on the receiver's identity
// rather than on the resolved function, so forwarding is exactly
// wrong: `String(new Proxy(class {}, {}))` came back `class {}`,
// which is not NativeFunction syntax at all.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
function add(a: number, b: number): number {
  return a + b;
}
const pfn: any = new Proxy(add as any, {});
const pcls: any = new Proxy(class C {} as any, {});
const pbound: any = new Proxy(add.bind(null) as any, {});
const parrow: any = new Proxy(((x: number) => x) as any, {});
const ptrap: any = new Proxy(add as any, { get: (t: any, k: any) => Reflect.get(t, k) });

console.log("String(fn proxy)   ", attempt(() => String(pfn)));
console.log("String(class proxy)", attempt(() => String(pcls)));
console.log("String(bound proxy)", attempt(() => String(pbound)));
console.log("String(arrow proxy)", attempt(() => String(parrow)));
console.log("String(get-trapped)", attempt(() => String(ptrap)));
console.log("concat             ", attempt(() => "" + pfn));
console.log("explicit .toString ", attempt(() => pfn.toString()));
console.log("toLocaleString     ", attempt(() => pfn.toLocaleString()));
console.log("Fn.p.toString.call ", attempt(() => Function.prototype.toString.call(pfn)));
// Step 5 — a non-callable proxy has no [[Call]], so it throws there.
console.log("non-callable throws", attempt(() =>
  Function.prototype.toString.call(new Proxy({} as any, {}))));
// …and plain String() on it never reaches §20.2.3.5 at all.
console.log("non-callable String", attempt(() => String(new Proxy({ a: 1 } as any, {}))));
// An own `toString` on the target still wins — it is resolved through
// [[Get]] before §20.2.3.5 is ever the function being called.
const tOverride: any = function named() {};
tOverride.toString = () => "custom";
console.log("target override    ", attempt(() => String(new Proxy(tOverride, {}))));
// Object.prototype.toString is a DIFFERENT function object (§8.3) —
// row 1, not row 13 — so it keeps answering its own §20.1.3.6 badge.
console.log("Object.p.toString  ", attempt(() => Object.prototype.toString.call(add as any)));
// The proxy's own `apply` property is an ordinary builtin, unmoved.
console.log("p.apply            ", attempt(() => String(pfn.apply)));
// A plain function is untouched by any of this.
console.log("plain fn unmoved   ", attempt(() => String(add).indexOf("[native code]") < 0));
