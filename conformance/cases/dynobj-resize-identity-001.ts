// dynobj store split (RFC 20260809): a class proto carrying
// constructor + 6 methods fills the initial 7-entry dense array; the
// next own write triggers resize. Pre-split, resize relocated the
// whole dict and only the writer's slot was updated — fresh
// `C.prototype` reads answered the old (freed) block, so the write
// "vanished" and identity split across owners. Post-split the header
// cell is address-stable: every owner sees the write and `===` holds.
class C {
  m1() {
    return 1;
  }
  m2() {
    return 2;
  }
  m3() {
    return 3;
  }
  m4() {
    return 4;
  }
  m5() {
    return 5;
  }
  m6() {
    return 6;
  }
}
(C.prototype as any).z = 9;
const s = new C();
console.log((C.prototype as any).z);
console.log(Object.getPrototypeOf(s) === C.prototype);
console.log((s as any).z);
(C.prototype as any).w = 10;
console.log((s as any).w);
console.log(s.m3());
