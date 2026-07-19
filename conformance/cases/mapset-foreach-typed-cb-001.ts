// Map/Set forEach with an explicitly typed callback param: entries are
// re-boxed as Any at the loop, so the call boundary must unbox into the
// typed lane (previously the NaN-box bits flowed raw into the i64 slot
// and the arithmetic below produced garbage like -1125899906842622).
const m = new Map<string, number>();
m.set("a", 1);
m.set("b", 2);
const out: number[] = [];
m.forEach(function (v: number) {
  out.push(v * 2);
});
console.log(out.join(","));
const s = new Set<number>();
s.add(3);
s.add(4);
const out2: number[] = [];
s.forEach(function (v: number) {
  out2.push(v + 1);
});
console.log(out2.join(","));
const names = new Map<string, string>();
names.set("x", "ex");
const collected: string[] = [];
names.forEach(function (v: string, k: string) {
  collected.push(k + ":" + v);
});
console.log(collected.join(","));
