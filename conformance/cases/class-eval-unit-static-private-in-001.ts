// K4-4 — `#x in o` for a STATIC private element of a class evaluated
// more than once: another evaluation's class object carries the same
// key and must still answer false (§13.10.1 asks for THIS evaluation's
// element); a shared-identity class keeps the bare own-entry probe.
function mk(): any {
  class K {
    static #f = 1;
    static #m() { return 1; }
    static hasF(o: any) { return #f in o; }
    static hasM(o: any) { return #m in o; }
    static viaArrow(o: any) { return [o].map((x: any) => #f in x)[0]; }
    static seen = #f in this;
  }
  return K;
}
const A = mk(), B = mk();
console.log(A.hasF(A), A.hasF(B), A.hasF({}), A.hasM(A), A.hasM(B), A.viaArrow(A), A.viaArrow(B), A.seen);
try { A.hasF(1); } catch (e) { console.log((e as any).constructor === TypeError); }
class T { static #q = 1; static has(o: any) { return #q in o; } }
console.log(T.has(T), T.has({}));
