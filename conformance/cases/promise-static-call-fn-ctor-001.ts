// §27.2.4.6 / .7 / .8 / Promise.try step 1 ask only IsConstructor of
// |this|, and NewPromiseCapability(C) reaches C through the construct
// channel whatever C is — so the settle statics called with a plain
// FUNCTION as the receiver run that function as the constructor and
// hand its executor the capability's resolving pair (the test262
// `Promise/reject/capability-invocation` shape). Before, the settle
// arms admitted only the builtin and its class heirs, and any other
// constructor threw the step-1 TypeError (rotation 650). A non-
// constructor receiver keeps that TypeError.
var seen: any[] = [];
var P: any = function (executor: any) { seen.push("ctor"); executor(function (v: any) { seen.push(["res", v]); }, function (e: any) { seen.push(["rej", e]); }); };
var r1 = (Promise.resolve as any).call(P, 7);
var r2 = (Promise.reject as any).call(P, 8);
console.log(JSON.stringify(seen), typeof r1, typeof r2);
var r3 = (Promise.all as any).call(P, []);
console.log(JSON.stringify(seen), typeof r3);
var w = (Promise.withResolvers as any).call(P);
console.log(typeof w, typeof w.resolve, JSON.stringify(seen)); w.resolve(9); console.log(JSON.stringify(seen));
var t = (Promise.try as any).call(P, function () { return 10; });
console.log(typeof t, JSON.stringify(seen));
var t2 = (Promise.try as any).call(P, function () { throw 11; });
console.log(typeof t2, JSON.stringify(seen));
var b = (Promise.reject as any).call(Promise, 2);
console.log(typeof b, b instanceof Promise);
try { (Promise.reject as any).call(undefined, 1); console.log("no throw"); } catch (e) { console.log("undef err", String(e)); }
try { (Promise.resolve as any).call(5, 1); console.log("no throw"); } catch (e) { console.log("prim err", e instanceof TypeError); }
try { (Promise.withResolvers as any).call({}); console.log("no throw"); } catch (e) { console.log("obj err", e instanceof TypeError); }
b.catch(function () {});
