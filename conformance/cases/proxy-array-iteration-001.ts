// §23.1.5.1 — iterating a Proxy whose target is an array runs
// Array.prototype[Symbol.iterator] with the proxy as `this`, so every
// step is a `length` read and an index read through the traps.
const log: string[] = [];
const p: any = new Proxy([1, 2, 3], {
  get(t: any, k: any) {
    if (typeof k === "string") log.push(k);
    return Reflect.get(t, k);
  },
});
const out: number[] = [];
for (const v of p) out.push(v);
console.log("forof", out.join(","));
console.log("reads", log.join(","));
console.log("from", Array.from(p).join(","));
console.log("spread", [...p].join(","));
console.log("length", p.length);
const growing: any[] = [1];
const gp: any = new Proxy(growing, {});
const seen: number[] = [];
for (const v of gp) {
  seen.push(v);
  if (seen.length === 1) growing.push(2);
  if (seen.length > 5) break;
}
console.log("length re-read each step", seen.join(","));
const objProxy: any = new Proxy({}, {});
try {
  for (const _v of objProxy) console.log("unreachable");
  console.log("no throw");
} catch (e: any) {
  console.log("plain object proxy", e instanceof TypeError ? "TypeError" : "other");
}
