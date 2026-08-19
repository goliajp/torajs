// §27.2.4.7/.6 step 1 — a detached `Promise.resolve` / `.reject`
// cell reads |this| as the constructor: `.apply(Promise, args)` /
// `.call(Promise, v)` and the member spelling all run the real
// settle (the reified cell carries the receiver channel), while the
// bare detached call keeps the step-1 TypeError (this = undefined).
var r: any = (Promise as any).resolve;
var p: any = r.apply(Promise, [7]);
p.then(function (x: any) { console.log("apply", x); });
var q: any = r.call(Promise, 8);
q.then(function (x: any) { console.log("call", x); });
try {
  r(42);
  console.log("bare fulfilled (wrong)");
} catch (e: any) {
  console.log("bare throws", e instanceof TypeError);
}
var m: any = (Promise as any).resolve(9);
m.then(function (x: any) { console.log("member", x); });
var rj: any = (Promise as any).reject;
var s: any = rj.call(Promise, "boom");
s.catch(function (e: any) { console.log("reject", e); });
