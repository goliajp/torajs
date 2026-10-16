// An `any` written into an interface slot holds an instance of a class
// that has the interface's members without naming it in `implements`:
// the slot reads that class's own methods and fields.
interface I {
  m(): number;
}
interface P {
  x: number;
  m(): number;
}
class J implements I {
  m() {
    return 8;
  }
}
class W {
  m() {
    return 9;
  }
}
class W2 extends W {
  m() {
    return 10;
  }
}
class V {
  y = 2;
  m() {
    return this.y;
  }
}
class Q {
  x = 3;
  z = "q";
  m() {
    return this.x * 2;
  }
}
class R {
  m() {
    return 12;
  }
  n() {
    return 13;
  }
}
function iface(v: any) {
  try {
    let p: I | undefined = new J();
    p = v;
    return String(p?.m());
  } catch (e) {
    return (e as Error).constructor.name;
  }
}
function pface(v: any) {
  try {
    const p: P = v;
    return String(p.x) + ":" + String(p.m());
  } catch (e) {
    return (e as Error).constructor.name;
  }
}
console.log(iface(new W()), iface(new W2()), iface(new V()), iface(new R()), iface(new Q()));
console.log(iface(undefined), iface(4), iface("s"), iface([1]));
console.log(pface(new Q()), pface(new W()), pface(undefined));
const ws: any[] = [new W(), new V(), new R(), new W2()];
let sum = 0;
for (const w of ws) {
  const p: I = w;
  sum += p.m();
}
console.log(sum);
