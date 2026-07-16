// Struct reflection over an Any-typed field holding a runtime-minted
// ShortStr (s[0] through any) — Object.values / Object.entries / gOPD
// / console.log all decode the slot without leaking the
// materialization (field_slot_to_pair_owned / _anyv_borrowed).
class C {
  v: any;
  n: number;
  constructor(s: any) {
    this.v = s[0];
    this.n = 7;
  }
}
function mk(s: any): any {
  return new C(s);
}
const src: any = "hello";
const c = mk(src);
console.log(Object.values(c));
console.log(Object.entries(c));
const d = Object.getOwnPropertyDescriptor(c, "v");
console.log(d.value, d.writable, d.enumerable, d.configurable);
console.log(c);
// struct desc with a ShortStr value through the runtime-desc lane
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
Object.defineProperty(o, "k", mkd(src[1]));
console.log(o.k);
