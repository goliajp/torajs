// `.constructor` identity — the inline-eq spelling must not fold a
// user class name away, and a subclass instance's constructor
// resolves through its class prototype ahead of the builtin's.
class MyC {}
const c = new MyC();
const k = c.constructor;
console.log(k === MyC);
console.log(c.constructor === MyC);
console.log(MyC === c.constructor);
console.log(c.constructor === Object);
class MyMap extends Map {}
const m = new MyMap();
console.log(m.constructor === MyMap, m.constructor === Map);
const o = {};
console.log(o.constructor === Object);
const xs = [1];
console.log(xs.constructor === Array);
