// An `any` value returned from a function declared to return an object
// type or a class, or stored into a field declared with one (of a class,
// or of a declared object type a binding is annotated with), is handed
// on as it is (§10.2.1.12, §10.1.9): the caller and the field's readers
// read its members by name, a member it lacks reads `undefined`, a
// method it lacks is a TypeError when it is called, and it keeps its
// identity.
type Pt = { x: number; y: number };
class C {
  x = 1;
  m() {
    return "C" + this.x;
  }
}
class D {
  x = 2;
  m() {
    return "D" + this.x;
  }
}
function attempt(tag: string, f: () => unknown): void {
  try {
    console.log(tag, f());
  } catch (e) {
    console.log(tag, "threw", (e as Error).constructor.name);
  }
}
function retC(v: any): C {
  return v;
}
function retP(v: any): Pt {
  return v;
}
class H {
  c: C = new C();
  p: Pt = { x: 0, y: 0 };
  f: () => string = () => "f";
}
type Box = { p: Pt };
const vals: any[] = [new D(), { x: 3 }, { x: 4, y: 5 }, 6, null];
for (const v of vals) {
  attempt("return class", () => {
    const r = retC(v);
    return [r === v, r?.x].join(" ");
  });
  attempt("return class call", () => retC(v).m());
  attempt("return object", () => {
    const r = retP(v);
    return [r === v, r?.x, r?.y].join(" ");
  });
  attempt("field class", () => {
    const h = new H();
    h.c = v;
    return [h.c === v, h.c?.x].join(" ");
  });
  attempt("field object", () => {
    const h = new H();
    h.p = v;
    return [h.p === v, h.p?.y].join(" ");
  });
  attempt("field function", () => {
    const h = new H();
    h.f = v;
    return typeof h.f;
  });
  attempt("declared field", () => {
    const b: Box = { p: { x: 1, y: 1 } };
    b.p = v;
    return [b.p === v, b.p?.x].join(" ");
  });
}
