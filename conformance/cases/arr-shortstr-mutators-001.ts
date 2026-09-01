// ShortStr values flowing through array mutators / iterators / assign
// boundaries keep value semantics; the churn probes assert the
// materialization-leak side (546-02 batch arr).
const a: any = [];
a.push("ab");
a.unshift("cd");
console.log(a.join(","));
const t: string[] = ["x"];
(t as any).push("ab");
(t as any).unshift("cd");
(t as any).splice(1, 0, "ef");
console.log(t.join(","));
const src: any = ["ab", "cd"];
const dst: string[] = src;
console.log(dst.join(","));
const xs: any = ["ab", ["cd"], "ef"];
console.log(JSON.stringify(xs.flat(Infinity)));
for (const v of xs) console.log(typeof v === "string" ? v : "arr");
for (const k of xs.keys()) console.log(k);
for (const [k, v] of (["ab"] as any).entries()) console.log(k, v);
const s: any = ["ab", "cd"];
console.log(s.values().next().value);
