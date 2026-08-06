function* sg() { const got: any = yield 1; console.log("sg received:", got); yield 2; }
const s: any = sg();
const GP: any = Object.getPrototypeOf(Object.getPrototypeOf(s));
console.log("s.next   :", JSON.stringify(GP.next.call(s)));
console.log("s.next42 :", JSON.stringify(GP.next.call(s, 42)));   // arg must reach the yield
console.log("s.return :", JSON.stringify(GP.return.call(s, 99)));

function* sg2() { try { yield 1; } catch (e: any) { console.log("sg2 caught:", e); } }
const s2: any = sg2();
GP.next.call(s2);
console.log("s2.throw :", JSON.stringify(GP.throw.call(s2, "boom")));

// bad receivers on the SYNC trio must still throw
for (const bad of [undefined, null, 42, {}, GP] as any[]) {
  try { GP.next.call(bad); console.log("sync bad: NO THROW"); }
  catch (e: any) { console.log("sync bad:", e instanceof TypeError); }
}
// g.prototype is one hop — must still be refused
try { GP.next.call((sg as any).prototype); console.log("sync proto: NO THROW"); }
catch (e: any) { console.log("sync proto:", e instanceof TypeError); }

// async trio: live receiver steps, bad receiver rejects
async function* ag() { const got: any = yield 10; console.log("ag received:", got); yield 20; }
const a: any = ag();
const AGP: any = Object.getPrototypeOf(Object.getPrototypeOf(a));
AGP.next.call(a).then((r: any) => {
  console.log("a.next   :", JSON.stringify(r));
  return AGP.next.call(a, 7);
}).then((r: any) => {
  console.log("a.next7  :", JSON.stringify(r));
  return AGP.return.call(a, 5);
}).then((r: any) => {
  console.log("a.return :", JSON.stringify(r));
  return AGP.next.call(undefined).then(() => "NO REJECT", (e: any) => "rejected:" + (e instanceof TypeError));
}).then((r: any) => console.log("async bad:", r));
