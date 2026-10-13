// A function declared to return a number hands back, as it is, what a
// call answers when the callee declares an `any` return: a library
// call (`JSON.parse`, a `Map<string, any>` read), a class method and a
// function declared `(): any`.
function p(s: string): number { return JSON.parse(s) }
console.log(typeof p('"a"') + ":" + p('"a"'), p("3") + 1);
const m: Map<string, any> = new Map();
m.set("k", "v");
m.set("n", 5);
function mg(k: string): number { return m.get(k) }
console.log(typeof mg("k") + ":" + mg("k"), mg("n") * 2);
class C {
  m(): any { return "c" }
}
function cm(o: C): number { return o.m() }
console.log(typeof cm(new C()) + ":" + cm(new C()));
function anyRet(): any { return "s" }
function viaAny(): number { return anyRet() }
console.log(typeof viaAny() + ":" + viaAny());
function unknownRet(): unknown { return "u" }
function viaUnknown(): number { return unknownRet() as any }
console.log(typeof viaUnknown() + ":" + viaUnknown());
