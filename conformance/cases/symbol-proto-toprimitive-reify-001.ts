// §20.4.3.5 — Symbol.prototype[@@toPrimitive] is a real property:
// readable, reflected, deletable, redefinable. ToPrimitive reads
// whatever is there now, and falls to OrdinaryToPrimitive without it.
const SP: any = Symbol.prototype;
const TP = Symbol.toPrimitive;
const f = SP[TP];
console.log(typeof f, TP in SP, f.name, f.length);
const d = Object.getOwnPropertyDescriptor(SP, TP)!;
console.log(d.writable, d.enumerable, d.configurable);
const s = Symbol("k");
const w: any = Object(s);
console.log(w[TP]() === s, f.call(s) === s, f.call(w) === s);
try { f.call(86); } catch (e) { console.log((e as Error).constructor === TypeError); }
try { `${w}`; } catch (e) { console.log("string of wrapper", (e as Error).constructor === TypeError); }
Object.defineProperty(SP, TP, { value: null });
try { +w; } catch (e) { console.log("number after null", (e as Error).constructor === TypeError); }
console.log(`${w}`);
delete SP[TP];
console.log(TP in SP);
SP.valueOf = () => 7;
console.log(Number(w));
