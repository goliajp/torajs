// a plain function whose @@hasInstance is installed by defineProperty
function F(this: any) {}
Object.defineProperty(F, Symbol.hasInstance, { value: (x: any) => x === 1 });
console.log("C1", (1 as any) instanceof F);
console.log("C2", (2 as any) instanceof F);

// a fn-expr binding with a handler
const G: any = function (this: any) {};
Object.defineProperty(G, Symbol.hasInstance, { value: (_x: any) => true });
console.log("C3", ({} as any) instanceof G);

// no handler installed — the ordinary prototype walk must be unchanged
function H(this: any) {}
const h = new (H as any)();
console.log("C4", h instanceof H);
console.log("C5", ({} as any) instanceof H);

// arrow/closure binding, no handler
const A2: any = class {};
console.log("C6", new A2() instanceof A2);
