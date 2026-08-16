// Every cell inherits Object.prototype.toString. Map / Set / Promise
// arms own no toString of their own, and their mid-miss sentinel used
// to reach OrdinaryToPrimitive as if it were a value — its quiet-NaN
// bit pattern coerced to "NaN".
var m = new Map();
var s = new Set();
var p = Promise.resolve(1);

console.log("map String", String(m));
console.log("map tmpl", `${m}`);
console.log("map concat", m + "");
console.log("map toString", m.toString());
console.log("map toLocaleString", m.toLocaleString());

console.log("set String", String(s));
console.log("set toString", s.toString());

console.log("promise String", String(p));
console.log("promise toString", p.toString());

// the explicit borrow of the badge classifier still agrees
console.log("call", Object.prototype.toString.call(m), Object.prototype.toString.call(s));

// tags that own a toString keep it
var d = new Date(0);
console.log("date", d.toISOString());
var a = [1, 2];
console.log("arr", String(a), a.toString());
var o = { x: 1 };
console.log("obj", String(o));
