// §20.1.2.10 Object.groupBy / §24.2.2.4 Map.groupBy / §27.2.4.8
// Promise.withResolvers as VALUES — the call lanes were complete,
// but `.length` / `.name` / a bound reference need the ns-static
// reified cell. groupBy has no |this| step, so a detached call runs
// the real kernel; withResolvers' detached call has an undefined
// |this| which is not a constructor (step 1) — catchable TypeError.
console.log((Object.groupBy as any).length, (Object.groupBy as any).name);
console.log((Map.groupBy as any).length, (Map.groupBy as any).name);
console.log((Promise.withResolvers as any).length, (Promise.withResolvers as any).name);
const g: any = Object.groupBy;
console.log(JSON.stringify(g([1, 2, 3, 4], (x: any) => (x % 2 === 0 ? "even" : "odd"))));
const mg: any = Map.groupBy;
const m = mg([1, 2, 3], (x: any) => (x < 3 ? "lo" : "hi"));
console.log(m.get("lo"), m.get("hi"));
const w: any = Promise.withResolvers;
try {
  w();
} catch (e: any) {
  console.log("threw", e.constructor.name);
}
console.log("done");
