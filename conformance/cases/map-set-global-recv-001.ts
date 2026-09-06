// A promoted Map / Set global read from a NAMED FN body finds a typed
// method lane. The shared receiver-face resolver answered the Ident
// spelling from the local table alone, and a promoted global is not in
// it — so every typed lane declined and the call reached the
// dispatcher cascade's terminal panic ("unsupported member call shape:
// get"), while the identical call on main, through a parameter, or on
// a fn-local Map all worked.
const m: Map<string, number> = new Map<string, number>();
m.set("k", 7);
m.set("j", 2);

function viaGlobal(): number {
  return (m.get("k") as number) + 1;
}
function viaSize(): number {
  return m.size;
}
function viaHas(): boolean {
  return m.has("j") && !m.has("zz");
}
console.log(viaGlobal(), viaSize(), viaHas());

// the iteration face of the same receiver
function viaKeys(): string {
  let out = "";
  for (const k of m.keys()) {
    out += k;
  }
  return out;
}
function viaForEach(): number {
  let sum = 0;
  m.forEach((v: number): void => {
    sum += v;
  });
  return sum;
}
console.log(viaKeys(), viaForEach());

// the Set half of the family
const s: Set<number> = new Set<number>();
s.add(7);
s.add(9);
function viaSet(): string {
  return s.size + ":" + s.has(7) + ":" + s.has(3);
}
console.log(viaSet());

// controls: a fn-LOCAL binding of the same name shadows the global and
// keeps its own type, and the main-side reads keep working
function shadowed(): number {
  const m: Map<string, number> = new Map<string, number>();
  m.set("k", 41);
  return (m.get("k") as number) + 1;
}
console.log(shadowed(), (m.get("j") as number), s.has(9));

// a Date global, which already had its lane — it must stay on it
const d: Date = new Date(0);
function viaDate(): number {
  return d.getTime();
}
console.log(viaDate());
