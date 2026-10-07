// a computed key inside an arrow reads a binding the arrow captures
function f() {
  const K = "a" + "b";
  const g = () => ({ [K]: 1, get [K + "x"]() { return 2; } });
  return g();
}
const r: any = f();
console.log(r.ab, r.abx);
class C {
  k = "z";
  m() {
    const self = this;
    return { [self.k]: 3 };
  }
}
console.log((new C().m() as any).z);
