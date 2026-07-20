// sweep appeared @ rotation 169 (test262 S15.3.4_A4) — methods a
// prototype inherits from Object.prototype must reify as the SAME
// function object across families.
console.log(Function.prototype.valueOf === Object.prototype.valueOf);
console.log(Array.prototype.valueOf === Object.prototype.valueOf);
console.log(Function.prototype.hasOwnProperty === Object.prototype.hasOwnProperty);
console.log(Array.prototype.isPrototypeOf === Object.prototype.isPrototypeOf);
console.log(Function.prototype.propertyIsEnumerable === Object.prototype.propertyIsEnumerable);
// own implementations stay distinct
console.log(Function.prototype.toString === Object.prototype.toString);
console.log(String.prototype.valueOf === Object.prototype.valueOf);
console.log(Date.prototype.valueOf === Object.prototype.valueOf);
console.log(Array.prototype.toLocaleString === Object.prototype.toLocaleString);
console.log(String.prototype.toLocaleString === Object.prototype.toLocaleString);
// delegated cell still dispatches correctly
const f: any = function () {};
console.log(f.hasOwnProperty("name"));
