// §7.1.1.1 OrdinaryToPrimitive is Get(O, name) → IsCallable → Call.
// The shapes that separate a real Get from a method dispatch: a
// non-callable own entry skips the name, an accessor runs its getter,
// a prototype's non-callable data patch is skipped too, and a null
// prototype leaves nothing to call.
console.log(String({ toString: undefined, valueOf() { return 1; } }));
console.log(String({ toString: 42, valueOf() { return "v"; } }));
console.log(`${{ get toString() { return () => "getter"; } }}`);
console.log(String({ [Symbol.toPrimitive](h: string) { return "hook:" + h; } }));
console.log(`${[{ toString() { return "T"; } }]}`);
console.log("ab".replace({ toString() { return "a"; } } as any, "Z"));
console.log(({ valueOf() { return 2; } } as any) < 3, ({ valueOf() { return 4; } } as any) + 1);
const xs: any = [1];
xs.length = { valueOf() { return 0; } } as any;
console.log(xs.length, Symbol.for({ toString() { return "k"; } } as any).description);
console.log(String(new Proxy({}, { get(t, k) { return k === "toString" ? () => "P" : undefined; } })));
try { String(Object.create(null)); } catch (e) { console.log("null proto", e instanceof TypeError); }
class D extends Date { toString() { return "S"; } }
console.log(String(new D(0)), String(Number.prototype), String(new (class extends Array { toString() { return "A"; } })()));
const f = function named() { return 1; };
console.log(`${f}`.slice(0, 14));
