// A method read as a value through an interface and stored as a Map
// key or into a Set: the collection hands the function back, and a call
// through the key's declared type reads the return the interface
// spells, so the interface keeps it. A lookup (`has`) only compares.
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
  const reg = new Map<() => number, string>();
  reg.set(j.m, "k");
  reg.forEach((v: string, k: () => number) => console.log(v, k() + 1));
  const st = new Set<() => number>();
  st.add(j.m);
  for (const k of st) console.log(k() * 2);
  console.log(reg.has(j.m), st.has(j.m), new S(raw).m() + 1);
}
run(JSON.parse('{"x":2,"y":3}'));
