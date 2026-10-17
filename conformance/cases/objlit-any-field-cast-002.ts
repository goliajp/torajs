// An `any` member of an object literal whose declared type is an
// object layout, where the declared type is read off the receiver of a
// member assignment (a type alias, an object type literal) or off a
// class method's parameter, and where the value carries a fraction:
// the member takes the value as the slot's readers read it.
type T = { x: number };
type W = { t: T };
type N = { w: W };
const vals: any[] = [JSON.parse('{"x":1.5}'), { x: 2 }, JSON.parse('{"x":7}')];
function tryit(f: () => string): string {
  try {
    return f();
  } catch (e) {
    return (e as Error).constructor.name;
  }
}
class C {
  w: W;
  constructor(w: W) {
    this.w = w;
  }
  m(w: W): number {
    return w.t.x;
  }
}
function arg(w: W): number {
  return w.t.x;
}
function ret(v: any): W {
  return { t: v };
}
const out: string[] = [];
for (const v of vals) {
  out.push(tryit(() => { const n: N = { w: { t: { x: 2 } } }; n.w = { t: v }; return String(n.w.t.x); }));
  out.push(tryit(() => { const m: { w: W } = { w: { t: { x: 3 } } }; m.w = { t: v }; return String(m.w.t.x); }));
  out.push(tryit(() => { const w: W = { t: v }; return String(w.t.x); }));
  out.push(tryit(() => String(arg({ t: v }))));
  out.push(tryit(() => String(ret(v).t.x)));
  out.push(tryit(() => String(new C({ t: v }).w.t.x)));
  out.push(tryit(() => String(new C({ t: { x: 1 } }).m({ t: v }))));
}
console.log(out.join(" "));
