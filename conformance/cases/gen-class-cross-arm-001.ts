// The class BINDING itself crossing a yield (the t262
// cpn-class-decl-*-yield tail assertions): the capturing lane's
// minted class value is born inside one state arm, so a cross-yield
// read (`C[yield]()` / `new C()` after a later resume) must go
// through the state-machine field the lift bridges (`this.C = C`).
function* g() {
  class C {
    [yield 1]() {
      return 'inst';
    }
    static [yield 2]() {
      return 'stat';
    }
  }
  let c = new C();
  console.log(c[yield 3]());
  console.log((C as any)[yield 4]());
}
const it = g();
it.next();
it.next('a');
it.next('b');
it.next('a');
it.next('b');
