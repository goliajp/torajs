// §15.7.14 — a ComputedPropertyName evaluates during
// ClassDefinitionEvaluation, before the class binding is initialized,
// so a key that reads the class's own name throws. tr threw correctly
// but also printed a spurious `unknown identifier` warning, because
// the dead-zone mark the checker consults is filled after the desugar
// has moved the key expression out of the class body.
function f() {
  try { class C { static [C.name] = 1; } } catch (e: any) { console.log("caught", e.constructor.name); }
}
f();
try { class D { [D.name]() { return 1; } } } catch (e: any) { console.log("caught2", e.constructor.name); }
// the INITIALIZER of a static field runs after the binding exists, so
// reading the name there is fine
class E { static self = 0; static later = (E as any).self + 1; }
console.log("init", E.later);
