// RFC 20260912-global-var-cell — an implicit global's FIRST write over a
// property the program already defined on the global object is §9.1.1.4.6
// [[Set]] on that property: the binding takes the property (attributes
// kept) and the property takes the value. The bind kernel used to copy
// the property's old value back into the slot after the store, so `q = 3`
// read back 0 (and an `undefined`-valued property made `q + 1` NaN).
// The `x = x` an init-less eval `var` hoists to is the other direction:
// nothing was stored, the property's value is the binding's.
// oracle: node script goal with `__t262_evalScript = s => vm.runInThisContext(s)`.
Object.defineProperty(this, "q", { value: 0, writable: true, enumerable: true, configurable: false });
function g() { q = 3; return q + 1; }
console.log(g(), q, this.q, Object.getOwnPropertyDescriptor(this, "q").configurable);
Object.defineProperty(this, "u", { value: undefined, writable: true, enumerable: true, configurable: false });
function h() { u = 5; return u + 1; }
console.log(h(), u, this.u);
Object.defineProperty(this, "ev", { value: 7, writable: true, enumerable: true, configurable: false });
__t262_evalScript('var ev;');
console.log(ev, this.ev);
function s() { __t262_evalScript('var w = 4; w = w + 1; console.log(w, globalThis.w)'); }
s();
console.log(w, typeof w);
