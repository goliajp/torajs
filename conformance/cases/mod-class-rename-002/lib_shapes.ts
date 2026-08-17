// One hidden colliding class per baked-artifact family: computed
// member keys, generator methods (forwarder decls + super calls),
// async methods, statics (field + method + block), and type
// annotations naming the class.
const kk = "dyn";
class C2 { [kk]() { return 8; } }
export function viaComputed() { return (new C2() as any).dyn(); }
class G0 { *g() { yield 3; } }
export function viaGen() { return new G0().g().next().value; }
class P0 { m() { return 4; } }
class S0 extends P0 { *g() { yield super.m(); } }
export function viaSuper() { return new S0().g().next().value; }
class A0 { async am() { return 6; } }
export async function viaAsync() { return await new A0().am(); }
class S1 {
  static sv = 11;
  static sm() { return S1.sv + 1; }
  static { S1.sv += 1; }
}
export function viaStatic() { return S1.sm(); }
class T1 { n = 5; }
function take(t: T1): number { return t.n; }
export function viaAnn() { const x: T1 = new T1(); return take(x); }
