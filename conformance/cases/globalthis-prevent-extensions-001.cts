// RFC 20260912-global-var-cell — the global object is an ordinary
// object to the integrity kernels: `Object.preventExtensions(this)` makes
// it non-extensible (a sloppy implicit-global write then creates
// nothing, §6.2.5.6; a Script declaring a new `var` is refused, §16.1.7
// step 12.b), and `isFrozen` / `isSealed` answer for its attributes, not
// for an immortality flag. The singleton was marked `FLAG_STATIC_LITERAL`
// for rc-immortality, which the integrity kernels read as `.rodata`.
// oracle: node script goal with `__t262_evalScript = s => vm.runInThisContext(s)`.
console.log(Object.isExtensible(this), Object.isFrozen(this), Object.isSealed(this));
Object.preventExtensions(this);
console.log(Object.isExtensible(this), Object.isExtensible(globalThis));
function w() { zz = 1; return typeof zz; }
console.log(w());
var declared = 1;
declared = 2;
console.log(declared, this.declared);
function decl() { __t262_evalScript('var late;'); }
try { decl(); } catch (e) { console.log(e.constructor.name); }
console.log(typeof late);
