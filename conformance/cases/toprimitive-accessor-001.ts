// §7.1.1 step 1.a GetMethod(input, @@toPrimitive) is a real Get, so an
// accessor-shaped hook has to run its getter. The symbol probe answers
// an ACCESSOR sentinel; this lane read it as a VALUE — neither
// undefined nor null, so it slipped past the nullish gate and reached
// callable_entry, which boxed it into something no closure lookup
// recognises. Every accessor form threw "Symbol.toPrimitive is not a
// function" with the getter never running.

// own accessor, both hints
const o1: any = {};
Object.defineProperty(o1, Symbol.toPrimitive, {
  get() { console.log("GET own"); return (h: string) => "P:" + h; },
});
console.log(`${o1}`); // GET own / P:string
console.log(o1 + 1); // GET own / P:default1
console.log(Number(o1)); // GET own / NaN (P:number is not numeric)

// inherited accessor
const proto: any = {};
Object.defineProperty(proto, Symbol.toPrimitive, {
  get() { return () => "Inherited"; },
});
console.log(`${Object.create(proto)}`); // Inherited

// class getter syntax
class C {
  get [Symbol.toPrimitive]() { return (h: string) => "K:" + h; }
}
console.log(`${new C()}`); // K:string

// a throwing getter propagates instead of being swallowed
const o2: any = {};
Object.defineProperty(o2, Symbol.toPrimitive, {
  get() { throw new Error("boom"); },
});
try {
  console.log(`${o2}`);
} catch (e: any) {
  console.log("caught", e.message);
} // caught boom

// a getter answering nullish means "no hook" — OrdinaryToPrimitive runs
const o3: any = { toString() { return "ordinary"; } };
Object.defineProperty(o3, Symbol.toPrimitive, { get() { return undefined; } });
console.log(`${o3}`); // ordinary

// repeated reads stay correct (the getter's hook is released each time)
const o4: any = {};
Object.defineProperty(o4, Symbol.toPrimitive, { get() { return () => "X"; } });
console.log(`${o4}`, `${o4}`, `${o4}`); // X X X

// data-property and no-hook forms are untouched
const o5: any = { [Symbol.toPrimitive]: (h: string) => "D:" + h };
console.log(`${o5}`); // D:string
console.log(`${{ toString() { return "plain"; } }}`); // plain
