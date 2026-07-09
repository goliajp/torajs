// chunk 744 - struct cells reached through any resolve field reads via
// the runtime class-layout reflection probe (pre-fix a sid the
// compile-time IC couldn't see - a Pass 2 fresh literal in a
// later-lowered fn - answered silent undefined), and
// Object.fromEntries accepts struct entries per ES Get(entry,"0"/"1")
function readAny(v: any): void {
  console.log(v.x, v["x"], v[0]);
}
readAny({ x: 7, 0: "zero" });

const b: any = { 0: "a" } as any;
console.log(b[0]);

// alias-typed struct through any (IC lane regression pin)
type P = { x: number };
function readP(v: any): void {
  console.log(v.x);
}
const lit: P = { x: 4 };
readP(lit);

// fromEntries: struct entry (inline as-any literal), short entry,
// mixed with pair-array; owned literal receiver released
const e: any = { 0: "a", 1: 1 } as any;
const o = Object.fromEntries([e]);
console.log(o.a);
const m = Object.fromEntries([["p", 2] as any, { 0: "q", 1: 3 } as any]);
console.log(m.p, m.q);

// Set of struct entries
const st = new Set<any>();
st.add(e);
const r = Object.fromEntries(st);
console.log(r.a);

// absent field through the reflection probe answers undefined
const s: any = { x: 1 } as any;
console.log(s.missing === undefined);
