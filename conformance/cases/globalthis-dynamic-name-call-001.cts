// RFC 20260911-global-object-record — a call whose callee is a name the
// program never declares: §6.2.5.5 GetValue reads it from the global
// object (or throws the ReferenceError), and §13.3.6.2 CALLS what it
// read. The undeclared-callee claim used to return the read alone, so
// `dyn(4)` answered the function itself when the object carried `dyn`.
// oracle: node script goal.
this.dyn = function (n) { return n + 1; };
console.log(dyn(4), typeof dyn);
Object.defineProperty(this, "acc", { get: function () { return function (n) { return n * 3; }; }, configurable: true });
console.log(acc(2));
try { nope(1); } catch (e) { console.log(e.constructor.name); }
function later() { return viaFn(10); }
this.viaFn = function (n) { return n - 1; };
console.log(later());
