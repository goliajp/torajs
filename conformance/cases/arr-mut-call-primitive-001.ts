// Mutator family borrowed onto primitive receivers — the len = 0
// empty-receiver shape (test262 Array/prototype/*/call-with-boolean).
console.log(Array.prototype.pop.call(true), Array.prototype.shift.call(false));
console.log(Array.prototype.push.call(true), Array.prototype.push.call(true, 9, 9), Array.prototype.unshift.call(false, 1));
let rv = Array.prototype.reverse.call(true);
console.log(rv instanceof Boolean, String(rv));
let fl = Array.prototype.fill.call(true, 7);
console.log(fl instanceof Boolean);
let cw = Array.prototype.copyWithin.call(false, 0, 1);
console.log(cw instanceof Boolean);
let so = Array.prototype.sort.call(5);
console.log(so instanceof Number, String(so));
let sp = Array.prototype.splice.call(true, 0, 0);
console.log(Array.isArray(sp), sp.length);
let pn = Array.prototype.pop.call(42);
console.log(pn, Array.prototype.push.call(3.5, 1));
// real-array lanes unchanged
let xs = [1, 2];
console.log(xs.pop(), xs.push(7), xs.length);
