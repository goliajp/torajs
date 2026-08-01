// RFC 20260801-ns-object-value — Math namespace object as a value.

// 1. value binding + identity
const m: any = Math;
console.log(m === Math);

// 2. return Math from a closure
const f = function () { return Math; };
console.log(f() === Math);

// 3. toString badge
console.log(Object.prototype.toString.call(Math));

// 4. member read through the escaped value
console.log(m.abs(-5));
console.log(m.PI);
console.log(m.max(3, 9));

// 5. Math passed as an arg and back
function id(x: any) { return x; }
console.log(id(Math) === Math);

// 6. typeof
console.log(typeof Math);

// 7. Array.isArray on the namespace object
console.log(Array.isArray(Math));

// 8. thisArg pass-through to a builtin HOF
let seen: any = null;
[11].every(function (v: any) { seen = this; return true; }, Math);
console.log(seen === Math);
