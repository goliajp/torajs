// §12.10 ASI — a bare FieldDefinition terminated by a line break
// (t262 after-same-line-*-asi family shape).
class C {
  *m() { return 42; } a
  b = 42;;
}
const c: any = new C();
console.log(c.a, c.b, c.m().next().value);

class D {
  x
  y = 3
  z
}
const d: any = new D();
console.log(d.x, d.y, d.z);
