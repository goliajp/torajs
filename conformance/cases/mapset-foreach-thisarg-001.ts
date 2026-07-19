// Map/Set forEach thisArg over TYPED receivers (§23.1.3.5 /
// §24.2.3.6 T): a this-using fn-expr callback promotes (knife 4
// mapset mirror) and receives the boxed thisArg as its leading
// __this arg via sig_skip alignment. Covers binding-shaped and
// inline-literal thisArg (the literal's owned temp must stay alive
// across every iteration), typed and seeded callback params, and a
// multi-entry walk.
const m = new Map<string, number>();
m.set("a", 1);
m.set("b", 2);
const out: any = [];
m.forEach(function (v: number) {
  out.push(v * this.mul);
}, { mul: 10 });
console.log(out.join(","));
const s = new Set<number>();
s.add(3);
s.add(4);
const out2: any = [];
const ctx2 = { add: 100 };
s.forEach(function (v) {
  out2.push(v + this.add);
}, ctx2);
console.log(out2.join(","));
const m2 = new Map<string, string>();
m2.set("k", "val");
m2.forEach(function (v, k) {
  console.log(this.tag, k, v);
}, { tag: "T:" });
