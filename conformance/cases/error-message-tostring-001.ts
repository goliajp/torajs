// §20.5.1.1 step 3 — Error-family constructors ToString a present
// message (single copy in the root ctor; subclasses forward through
// super). Pre-fix the checker rejected any non-string argument
// (`new Error(42)` → "expected String, got I64"). Absence (missing
// arg or explicit undefined) still defines no own message; a Symbol
// message throws (ToString semantics — the "" + coercion preserves
// what a String() call would lose).
console.log(new Error(42 as any).message);
console.log(new TypeError(7 as any).message);
console.log(new RangeError(null as any).message);
console.log(new SyntaxError(true as any).message);
console.log(new ReferenceError({ k: 1 } as any).message);
console.log((new SuppressedError("e" as any, "s" as any, 99 as any) as any).message);
console.log((new AggregateError([] as any, 3.5 as any) as any).message);
const e: any = new Error(123 as any);
console.log(e.message, e.stack.split("\n")[0]);
const u: any = new Error(undefined);
console.log(u.message === "", Object.hasOwn(u, "message"));
try {
  new TypeError(Symbol() as any);
} catch (er: any) {
  console.log("symbol threw", er instanceof TypeError);
}
console.log(new Error("plain").message);
