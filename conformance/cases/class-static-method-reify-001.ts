// RFC 20260717-class-first-class-value knife B cut 2 — static
// methods are own function-valued properties of the class object
// with the §10.2.10 attribute set; call surfaces (direct, any-lane)
// and enumeration stay intact.
class S {
  static sm() {
    return 2;
  }
  static sm2(n: number) {
    return n * 2;
  }
  m() {
    return 1;
  }
}
const sd = Object.getOwnPropertyDescriptor(S, "sm");
console.log(sd.configurable, sd.enumerable, sd.writable);
console.log(typeof sd.value);
console.log(S.sm(), S.sm2(21));
const f: any = S;
console.log(f.sm(), f.sm2(4));
console.log(Object.keys(S).length);
console.log(typeof S.sm);
