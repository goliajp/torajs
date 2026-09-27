// import()'s options argument (§13.3.10 EvaluateImportCall steps
// 10-11), evaluated after the specifier and validated after
// ToString(specifier): a non-object options value or `with` value, a
// non-string attribute value, or a getter throw rejects the promise.
// The `with` object is read as EnumerableOwnProperties — symbol keys
// and non-enumerable keys are skipped without a [[Get]]. tr supports
// no import attribute, so any entry rejects with a TypeError
// (AllImportAttributesSupported); bun loads `type: "json"` as a JSON
// module, which is why `main.expected` pins that one line.
const log: string[] = [];
function show(tag: string, p: Promise<any>): Promise<void> {
  return p.then((m: any) => { console.log(tag, "ok", m.default); },
                (e: any) => { console.log(tag, "reject", e.constructor.name); });
}
async function main(): Promise<void> {
  const withProxy = new Proxy({}, {
    ownKeys() { log.push("ownKeys"); return [Symbol("s"), "hidden", "absent"]; },
    getOwnPropertyDescriptor(_t: any, k: any) {
      log.push("gopd:" + String(k));
      return k === "hidden" ? { configurable: true, enumerable: false, value: 1 } : undefined;
    },
    get(_t: any, k: any) { log.push("get:" + String(k)); return "x"; },
  });
  await show("none", import("./lib.ts"));
  await show("undef", import("./lib.ts", undefined));
  await show("trailing", import("./lib.ts", undefined,));
  await show("prim", import("./lib.ts", 1 as any));
  await show("null", import("./lib.ts", null as any));
  await show("nowith", import("./lib.ts", {}));
  await show("withundef", import("./lib.ts", { with: undefined }));
  await show("withprim", import("./lib.ts", { with: "x" } as any));
  await show("empty", import("./lib.ts", { with: {} }));
  await show("key", import("./lib.ts", { with: { type: "json" } }));
  await show("nonstr", import("./lib.ts", { with: { type: 1 } } as any));
  await show("proxy", import("./lib.ts", { with: withProxy }));
  console.log(log.join(","));
  const order: string[] = [];
  const p = import((order.push("a"), "./lib.ts"), (order.push("b"), undefined));
  console.log(order.join(","));
  await show("seq", p);
  let q: any;
  for (q = import("./lib.ts", "k" in {} || undefined); false; ) ;
  await show("forin", q);
  const getterThrows = { get with(): any { throw new RangeError("g"); } };
  await show("getthrow", import("./lib.ts", getterThrows));
}
main();
