// §27.2.4.3.1 steps 9-12 — every allSettled record is an ordinary
// object with {status, value} / {status, reason}. The dyn entries
// (an `any` iterable, the recv-first `.call` spelling) have no typed
// call site to mint a class-layout stamp, and the tag-0 class-shape
// record they used to build was 48 anonymous bytes: `r.status`
// through the any lane answered undefined and JSON.stringify agreed.
// A tag-less site now builds a plain dynobj record instead, readable
// by name like any ordinary object.
function later(v: any): any {
  return new Promise((res: any) => { Promise.resolve().then(() => res(v)); });
}
function laterRej(v: any): any {
  return new Promise((_res: any, rej: any) => { Promise.resolve().then(() => rej(v)); });
}
function mixed(): any {
  return [1, Promise.reject(2)];
}
function mixedPending(): any {
  return [later(1), laterRej("x"), 3];
}
async function main() {
  const rs: any = await Promise.allSettled(mixed());
  for (const r of rs) console.log(r.status, r.value, r.reason);
  const rp: any = await Promise.allSettled(mixedPending());
  for (const r of rp) console.log(r.status, r.value, r.reason);
  console.log(JSON.stringify(rp));
  const rc: any = await (Promise as any).allSettled.call(Promise, [later(5), "s"]);
  for (const r of rc) console.log(r.status, r.value, r.reason);
}
main();
