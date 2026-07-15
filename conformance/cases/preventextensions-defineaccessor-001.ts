// ES §B.2.2.2/3 — __defineGetter__ / __defineSetter__ on a
// preventExtensions'd DynObj literal. Regression fix: previously
// tr routed the ObjectLit-literal arg to Object.preventExtensions
// via the typed-struct box lane (tag Obj), so the downstream
// method_call_legacy_accessor guard rejected with "legacy accessor
// methods are not supported on this receiver" (2 test262
// Object/prototype/__defineGetter__|__defineSetter__/
// define-non-extensible.js). Routing ObjectLit-arg through
// lower_dynobj_init (mirror of Object.create's props arm) lands the
// receiver on the DynObj tag and lets dynobj_define enforce the
// §10.1.6.3 non-extensible new-key gate with bun's exact wording.

var noop = function() {};

// __defineGetter__ redefines an existing property on a
// preventExtensions'd literal: OK.
var g: any = Object.preventExtensions({ existing: null });
g.__defineGetter__('existing', function() { return 42; });
console.log("g.existing (getter):", g.existing);

// Same for __defineSetter__.
var captured: any = "unset";
var s: any = Object.preventExtensions({ existing: null });
s.__defineSetter__('existing', function(v: any) { captured = "set:" + v; });
s.existing = 7;
console.log("captured:", captured);

// Adding a brand-new property to a non-extensible DynObj via
// __defineGetter__ throws bun-parity TypeError.
var b: any = Object.preventExtensions({ existing: null });
try {
  b.__defineGetter__('brand new', noop);
  console.log("b add new getter: OK (BAD)");
} catch (e: any) {
  console.log("b add new getter throws:", e.message);
}

// Same for __defineSetter__ adding a new key.
try {
  b.__defineSetter__('brand new', noop);
  console.log("b add new setter: OK (BAD)");
} catch (e: any) {
  console.log("b add new setter throws:", e.message);
}
