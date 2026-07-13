// RFC 20260714-dstr-residual blade 4 — NamedEvaluation for anonymous
// class expressions (ES §8.4.5 / §15.5.5): a binding or destructuring
// default names the anonymous class; a named class expression keeps
// its self-name; a static `name` method shadows the reflection field.

// let binding names the anonymous class
let D = class {};
console.log("D:", D.name);

// self-name wins over the binding
let E2 = class Named {};
console.log("E2:", E2.name);

// destructuring default, param position
function fc([C = class {}]: any) {
  console.log("C:", C.name);
}
fc([]);

// destructuring default, let position — anonymous + self-named +
// static name() shadowing
let [cls = class {}, xCls = class X {}, xCls2 = class { static name() {} }] =
  [] as any;
console.log("cls:", cls.name, "|", xCls.name, "|", typeof xCls2.name);

// plain class declaration unaffected
class Plain {}
console.log("Plain:", Plain.name);
console.log("done");
