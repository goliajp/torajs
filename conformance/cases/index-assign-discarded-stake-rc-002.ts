// statement-position index stores into consuming receivers: a BigInt
// typed array, a fractional key on a string array, a setter and a proxy
const w = new BigInt64Array(4);
for (let i = 0; i < 200000; i++) w[i % 4] = BigInt(i);
console.log(w[0], w[3]);
const xs: string[] = ["a"];
for (let i = 0; i < 1000; i++) (xs as any)[1.5] = "s" + i;
console.log((xs as any)[1.5], xs.length);
let kept: any = null;
const o: any = { set v(x: any) { kept = x; } };
for (let i = 0; i < 1000; i++) o["v"] = { i };
console.log(kept.i);
const seen: any[] = [];
const p: any = new Proxy({}, { set(_t, _k, v) { seen.push(v); return true; } });
for (let i = 0; i < 3; i++) p["k" + i] = { i };
console.log(seen.map((x) => x.i).join(","));
