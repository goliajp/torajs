// §20.4.3.3 / §20.4.3.4 — the reified Symbol.prototype.toString /
// valueOf cells run thisSymbolValue: a Symbol receiver answers its
// descriptive string / itself, EVERY other receiver is a TypeError.
// The cells carry dedicated mids (tag-5 prototype alias) because the
// shared TO_STRING / VALUE_OF ids re-dispatch into the primitive
// fast arms (valueOf.call(0) answered 0 pre-fix).
let valueOf: any = (Symbol as any).prototype.valueOf;
let toString: any = (Symbol as any).prototype.toString;
let s: any = Symbol("k");
console.log(typeof valueOf, typeof toString);
console.log(valueOf.call(s) === s);
console.log(toString.call(s));
for (const v of [null, undefined, 0, "", {}, []] as any[]) {
  try { valueOf.call(v); console.log("NO-THROW", typeof v); }
  catch (e: any) { console.log(e instanceof TypeError); }
}
try { toString.call(42); console.log("NO-THROW-TS"); }
catch (e: any) { console.log(e instanceof TypeError); }
// the plain symbol-receiver forms keep their fast arms
console.log(s.toString(), String(s.description));
