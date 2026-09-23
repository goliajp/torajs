// `forEach` re-boxes the entry and the call boundary converted the box
// into a declared `number` parameter with §7.1.4 — which turns the
// undefined tag into an ordinary NaN and spends the F64 sentinel the
// parameter could have carried. `typeof v` inside answered "number"
// where `m.get(k)` on the same entry answered "undefined".
const zs: number[] = [];

const m = new Map<string, number>();
m.set("a", zs[9]);
console.log("get", typeof m.get("a"), m.get("a") === undefined);
m.forEach((v, k) => console.log("map", k, typeof v, v === undefined));
for (const [k, v] of m) {
  console.log("map for-of", k, typeof v, v === undefined);
}

const s = new Set<number>();
s.add(zs[9]);
// Only the first parameter: §24.2.3.6 hands the member twice, but
// the width pass seeds the second one I64, which cannot spell the
// sentinel at all — a width gap, registered separately.
s.forEach((v) => console.log("set", typeof v, v === undefined));

// Live entries keep the plain number lane, arithmetic and all.
const live = new Map<string, number>();
live.set("b", 41);
live.forEach((v) => console.log("live", typeof v, v + 1));

// `Number(undefined)` is still NaN — the slot kernel is not ToNumber.
console.log("ToNumber", Number(undefined), Number(zs[9]));
