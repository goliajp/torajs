// §7.3.34 InitializeInstanceElements — a class's private methods and
// accessors belong to an object only once the class has installed
// them: right after allocation for a base class, right after
// `super(…)` returns for a derived one.
function attempt(label: string, f: () => unknown) {
  try {
    console.log(label, "->", f());
  } catch (e) {
    console.log(label, "throws", e instanceof TypeError);
  }
}

class CallsInCtor {
  constructor() {
    (this as any).f();
  }
}
class MethodBeforeSuper extends CallsInCtor {
  f() {
    return this.#m();
  }
  #m() {
    return 42;
  }
}
class GetterBeforeSuper extends CallsInCtor {
  f() {
    return this.#g;
  }
  get #g() {
    return 1;
  }
}
class SetterBeforeSuper extends CallsInCtor {
  f() {
    this.#s = 1;
  }
  set #s(v: number) {}
}
attempt("method before super returns", () => new MethodBeforeSuper());
attempt("getter before super returns", () => new GetterBeforeSuper());
attempt("setter before super returns", () => new SetterBeforeSuper());

class CallsInField {
  x = (this as any).f();
}
class MethodFromField extends CallsInField {
  f() {
    return this.#m();
  }
  #m() {
    return 1;
  }
}
attempt("method from parent field initializer", () => new MethodFromField());

class Base {
  #m() {
    return "base";
  }
  invoke(o: any) {
    return o.#m();
  }
  own() {
    return this.#m();
  }
}
class Derived extends Base {
  #n() {
    return "derived";
  }
  both() {
    return this.own() + "+" + this.#n();
  }
}
const b = new Base();
const d = new Derived();
attempt("base own", () => b.own());
attempt("derived both", () => d.both());
attempt("base method on derived", () => b.invoke(d));
attempt("prototype-only object", () => b.invoke(Object.create(Base.prototype)));
attempt("plain object", () => b.invoke({}));
let n = 0;
for (let i = 0; i < 1000; i++) n += new Derived().both().length;
console.log("loop", n);

class StaticRef {
  #f() {
    return 42;
  }
  static g(this: any) {
    return this.#f();
  }
  get ref() {
    return this.#f;
  }
  viaCall(o: any) {
    return this.#f.call(o);
  }
}
attempt("static method on instance", () => StaticRef.g.call(new StaticRef()));
attempt("static method on class", () => StaticRef.g());
const sr = new StaticRef();
attempt("method value identity", () => sr.ref === sr.ref);
attempt("extracted method on foreign receiver", () => sr.viaCall({}));
function make(): StaticRef {
  return new StaticRef();
}
attempt("call result receiver", () => (make() as any).ref !== undefined);
let rounds = 0;
for (let i = 0; i < 200000; i++) rounds += sr.viaCall(sr) - 41;
console.log("churn", rounds);
