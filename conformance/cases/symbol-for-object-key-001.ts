// §20.4.2.2 Symbol.for step 1 — ToString(key) runs the key's own
// toString, typed struct or `any` object alike.
const o = { toString() { return "bcd"; } };
console.log(Symbol.for(o as any).description);
const q: any = { toString() { return "bcd"; } };
console.log(Symbol.for(q).description, Symbol.for(q) === Symbol.for("bcd"));
const a: any = [1, 2];
console.log(Symbol.for(a).description);
console.log(Symbol.for(12).description, Symbol.for(true).description, Symbol.for(null).description);
console.log(Symbol.for(o, 1).description, Symbol.for(12) === Symbol.for("12"));
const n: number = 3.5;
console.log(Symbol.keyFor(Symbol.for(n)));
