// §20.4.3.5 Symbol.prototype[@@toPrimitive] answers thisSymbolValue, so
// every ToPrimitive over a Symbol wrapper unwraps to the Symbol — and
// ToString / ToNumber of a Symbol throw (§7.1.17 step 2, §7.1.4 step 2).
// Only String(sym) on the PRIMITIVE answers its descriptive string.
const w: any = Object(Symbol("d"));
function tryIt(label: string, f: () => unknown) {
  try {
    console.log(label, String(f()));
  } catch (e) {
    console.log(label, (e as any).constructor.name);
  }
}
tryIt("String", () => String(w));
tryIt("template", () => `${w}`);
tryIt("concat", () => "" + w);
tryIt("unary+", () => +w);
tryIt("indexOf", () => "abc".indexOf(w));
tryIt("toString()", () => w.toString());
tryIt("description", () => w.description);
tryIt("prim", () => String(Symbol("d")));
