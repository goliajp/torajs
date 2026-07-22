// two-arg .then(onOk, onErr) over Any-inner and Array-inner
// receivers (rotation 184 — checker gate widened to mirror the
// 1-arg P10.7 / P10.2-A4 lanes). Top-level awaits serialize the
// chains so output order is source order on every engine.
const mixed: any[] = [1, Promise.resolve(2), "x"];
await Promise.all(mixed).then(
  (vs: any) => { console.log("ok", vs.length); },
  (e: any) => { console.log("err", e); }
);
const p: Promise<any> = Promise.resolve(42);
await p.then((v: any) => console.log("v", v), (e: any) => console.log("e", e));
const q: Promise<any> = Promise.reject("boom");
await q.then((v: any) => console.log("never", v), (e: any) => console.log("caught", e));
const nums: Promise<number> = Promise.resolve(7);
await nums.then((n: number) => n + 1, (e: number) => e).then((m: any) => console.log("m", m));
