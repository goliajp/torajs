// An interface method written as an arrow over an `any` read, taken as
// a value and stored as a Map key and into a Set: every call through
// the declared `() => number` signature reads the fraction the arrow
// returned.

interface J {
  m(): number;
}
class S implements J {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
function run(raw: any) {
  const j: J = { m: () => raw.y };
  console.log(j.m());
  const reg = new Map<() => number, string>();
  reg.set(j.m, "k");
  reg.forEach((v: string, k: () => number) => console.log(v, k() + 1));
  const st = new Set<() => number>();
  st.add(j.m);
  for (const k of st) console.log(k() * 2);
  console.log(reg.has(j.m), st.has(j.m), new S(raw).m() + 1);
}
run(JSON.parse('{"x":2.5,"y":3.5}'));
