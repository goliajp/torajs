// A Promise settle static reached with an explicit receiver
// (`Promise.reject.call(P, v)`, `.apply`, `.bind`) runs
// NewPromiseCapability(|this|) through the construct channel, and
// |this| may be any CLASS object — §27.2.4.6 step 1 asks only
// IsConstructor. A class factory only gets a boxed adapter when the
// program constructs from a value, and this spelling was not one of
// the signals, so a class with no Promise in its chain stopped at
// "this constructor cannot be reached through a runtime value yet"
// (rotation 650). The spelling now arms the adapters like
// `Reflect.construct` does; the `as any` cast the checker asks for
// is peeled.
var seen: any[] = [];
class P { constructor(executor: any) { seen.push("ctor"); executor(function (v: any) { seen.push(["res", v]); }, function (e: any) { seen.push(["rej", e]); }); } }
var a = (Promise.reject as any).call(P, 1); console.log("A", typeof a, JSON.stringify(seen));
var b = (Promise.resolve as any).call(P, 2); console.log("B", typeof b, JSON.stringify(seen));
var w = (Promise.withResolvers as any).call(P); console.log("W", typeof w.promise, JSON.stringify(seen));
var d = (Promise.reject as any).bind(P)(4); console.log("D", typeof d, JSON.stringify(seen));
var g = (Promise.all as any).call(P, []); console.log("G", typeof g, JSON.stringify(seen));
class Q { constructor(executor: any) { seen.push("q"); executor(function () {}, function () {}); } }
var f: any = Promise.reject; var q = f.call(Q, 5); console.log("Q", typeof q, JSON.stringify(seen));
