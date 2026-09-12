// RFC 20260912-global-var-cell knife 5 — a Script evaluated INSIDE a
// function (`$262.evalScript`, runner spelling `__t262_evalScript`)
// runs §16.1.7 GlobalDeclarationInstantiation when it is CALLED,
// against the global object as it is then: its declarations do not
// exist before the call, a function over a non-configurable read-only
// property is refused (TypeError, steps 9-10) and a `let` over a
// non-configurable property is refused (SyntaxError, step 5.c) — in
// both refusals NO binding of the Script is created.
// oracle: node script goal with `__t262_evalScript = s => vm.runInThisContext(s)`,
// EXCEPT the `locked` line: V8 answers SyntaxError where §16.1.7 step
// 10.a.ii / CanDeclareGlobalFunction says TypeError (test262
// `script-decl-func-err-non-configurable` asserts TypeError), so the
// expected holds the spec's answer.
function declare() {
  __t262_evalScript('var fresh; function seven() { return 7 }');
}
console.log(typeof fresh, typeof seven);
declare();
console.log(typeof fresh, typeof seven);
var held = seven;
console.log(held(), Object.getOwnPropertyDescriptor(this, 'fresh').configurable, Object.getOwnPropertyDescriptor(this, 'seven').enumerable);
Object.defineProperty(this, 'locked', { configurable: false, value: 0, writable: false, enumerable: true });
function overLocked() {
  __t262_evalScript('var y; function locked() {}');
}
try { overLocked(); } catch (e) { console.log(e.constructor.name); }
console.log(typeof y, this.locked);
Object.defineProperty(this, 'fixed', { configurable: false });
function lexOverFixed() {
  __t262_evalScript('var z; let fixed;');
}
try { lexOverFixed(); } catch (e) { console.log(e.constructor.name); }
console.log(typeof z);
function again() {
  __t262_evalScript('var fresh; function seven() { return 70 }');
}
again();
console.log(held(), typeof globalThis.seven);
