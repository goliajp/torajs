// Map.groupBy with RUNTIME-built string keys (slice products; ShortStr
// encoded) — the key must survive the SameValueZero lookup and read
// back by content. Pre-fix the group_by loop never handed map_get the
// stake its contract consumes, so a runtime key died inside the
// lookup and the fresh-insert arm adopted a freed pointer:
// `h.get("xyz")` answered undefined (546-02 misc batch).
const xs: any = ["abcdef", "abcxyz", "abcdef"];
const g = Map.groupBy(xs, (x: any) => x.slice(0, 3));
console.log(g.size, JSON.stringify(g.get("abc")));
const h = Map.groupBy(xs, (x: any) => x.slice(3, 6));
console.log(h.size, JSON.stringify(h.get("def")), JSON.stringify(h.get("xyz")));
// Same-key hits exercise the bucket-exists arm (map_get's OWNED value
// return released, the bucket stays owned by the map alone).
const k = Map.groupBy(xs, (_: any, i: any) => i % 2);
console.log(k.size, JSON.stringify(k.get(0)), JSON.stringify(k.get(1)));
