// Object.defineProperty with a runtime (non-literal) descriptor:
// owned call-product desc temps are released after the helper borrows
// them; ident-bound descs stay alive (borrow, no drop). Guards the
// release_owned_temp wiring in emit_define_runtime_desc against both
// the leak (owned temp never dropped) and the over-release (ident
// borrow dropped -> UAF).
class D {
  value: any;
  enumerable: boolean;
  writable: boolean;
  configurable: boolean;
  constructor(v: any) {
    this.value = v;
    this.enumerable = true;
    this.writable = true;
    this.configurable = true;
  }
}
function mkd(v: any): any {
  return new D(v);
}
const o: any = {};
// owned temp desc (call product) — released after define
Object.defineProperty(o, "k", mkd(42));
console.log(o.k);
// ident-bound desc — borrowed, must stay alive after define
const dd = mkd(7);
Object.defineProperty(o, "m", dd);
console.log(o.m, dd.value);
const d = Object.getOwnPropertyDescriptor(o, "k");
console.log(d.value, d.writable, d.enumerable, d.configurable);
