// §22.1.1 step 1.a — explicit String(symbol) answers the
// SymbolDescriptiveString (the one legal Symbol stringify position).
const s1 = Symbol("desc");
const s2 = Symbol();
console.log(String(s1));
console.log(String(s2));
const anySym: any = Symbol("viaAny");
console.log(String(anySym));
console.log(`wrapped: ${String(s1)}`);
