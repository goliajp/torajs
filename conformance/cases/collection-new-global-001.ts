// An un-annotated `new Map()` / `new Set()` / `new WeakMap()` binding
// gets a global home. `desugar_builtin_new` rewrites `new Date` /
// `new Array` / `new RegExp` into shapes the promote arms already
// read, but not the four collections — so they reached neither the
// slot-shape table (no `Expr::New` arm) nor the `__new_C` factory
// spelling, and any named-fn read answered "unknown identifier".
const m = new Map<string, number>();
m.set("k", 7);
m.set("j", 2);

function viaMap(): number {
  return (m.get("k") as number) + m.size;
}
function viaHas(): boolean {
  return m.has("j") && !m.has("zz");
}
console.log(viaMap(), viaHas());

// no type arguments at all — the bare spelling
const bare = new Map();
bare.set("a", 1);
function viaBare(): number {
  return bare.size;
}
console.log(viaBare());

const s = new Set<number>();
s.add(7);
s.add(9);
s.add(7);
function viaSet(): string {
  return s.size + ":" + s.has(7) + ":" + s.has(3);
}
console.log(viaSet());

const key = { tag: "k" };
const w = new WeakMap<object, number>();
w.set(key, 5);
function viaWeak(): number {
  return (w.get(key) as number) + 1;
}
console.log(viaWeak(), w.has(key));

// iteration through the promoted receiver
function viaKeys(): string {
  let out = "";
  for (const k of m.keys()) {
    out += k;
  }
  return out;
}
console.log(viaKeys());

// controls: the builtins that already promoted must keep their shapes
const d = new Date(0);
const a = new Array<number>();
a.push(7);
function viaOthers(): string {
  return d.getTime() + ":" + a.length;
}
console.log(viaOthers());
