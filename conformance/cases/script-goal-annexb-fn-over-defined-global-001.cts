// A Script's block function over a global property the program defined
// (Annex B §B.3.3.3 in Script code, `annexB/language/global-code/
// *-existing-non-enumerable-global-init`): CreateGlobalVarBinding
// finds the property and leaves it — attributes included — and the
// block's evaluation is a [[Set]] of the function into it. Before the
// Script runs, the name resolves on the global object (§9.1.1.4
// GetBindingValue): `f` reads the defined value. Expected output is
// node's line for line with
// `$262.evalScript` = `vm.runInThisContext` (Script code). Before: the
// Script's Annex B `var` was bound in the prelude as a non-configurable
// written `var`, and the program's own `Object.defineProperty(this,
// "f", {configurable: true})` threw; and a read before the text ran
// answered the slot's `undefined` instead of the property.
var global = this;
Object.defineProperty(global, 'f', { value: 'x', enumerable: false, writable: true, configurable: true });
__t262_evalScript(`
console.log("s1", f, typeof f);
var d = Object.getOwnPropertyDescriptor(global, 'f');
console.log("s1", d.enumerable, d.writable, d.configurable);
`);
__t262_evalScript(`
{
  function f() { return 'inner declaration'; }
}
`);
__t262_evalScript(`
console.log("s3", f(), typeof f, global.f === f);
var d = Object.getOwnPropertyDescriptor(global, 'f');
console.log("s3", d.enumerable, d.writable, d.configurable);
`);
// the same read-before-bind for a plain `var` of a later Script
Object.defineProperty(global, 'q', { value: 'qx', enumerable: false, writable: true, configurable: true });
__t262_evalScript(`console.log("q1", q, typeof q);`);
__t262_evalScript(`var q;`);
__t262_evalScript(`
console.log("q3", q);
var d = Object.getOwnPropertyDescriptor(global, 'q');
console.log("q3", d.enumerable, d.writable, d.configurable);
`);
