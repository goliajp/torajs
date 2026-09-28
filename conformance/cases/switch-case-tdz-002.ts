// a read in an earlier clause of a binding a later clause declares is a
// dead-zone read whenever it happens; the checker must not call the name
// undeclared, and typeof does not shelter it
function r(f: () => string): string { try { return f(); } catch (e) { return (e as Error).constructor.name; } }
function early(x: number): string {
  switch (x) {
    case 1: return "one" + typeof u;
    case 2: const u = 7; return "two" + u;
    default: return "d";
  }
}
function same(x: number): string {
  switch (x) {
    case 1: { const q = 1; return "q" + q; }
    case 2: return "v" + v;
    case 3: let v = 3; return "three" + v;
  }
  return "none";
}
function viaDefault(x: number): string {
  switch (x) {
    default: return "d" + w;
    case 1: let w = 1; return "w" + w;
  }
}
console.log(r(() => early(1)), early(2), early(9));
console.log(r(() => same(2)), same(1), same(3));
console.log(r(() => viaDefault(5)), viaDefault(1));
