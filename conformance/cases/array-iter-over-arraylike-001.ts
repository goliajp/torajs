// §23.1.5.1 CreateArrayIterator over an array-like: the reified
// Array.prototype.{keys,values,entries}.call(obj) reads `length` and each
// index through Get, every step, and hands out %ArrayIteratorPrototype%.
var obj: any = { length: 2, 0: "a", 1: "b" };
var it: any = Array.prototype.keys.call(obj);
console.log(it.next().value, it.next().value, it.next().done);
var vs: any = Array.prototype.values.call(obj);
console.log(vs.next().value, vs.next().value, vs.next().done);
var es: any = Array.prototype.entries.call(obj);
console.log(JSON.stringify(es.next().value), JSON.stringify(es.next().value), es.next().done);
var ArrayIteratorProto = Object.getPrototypeOf([][Symbol.iterator]());
console.log(Object.getPrototypeOf(it) === ArrayIteratorProto, Object.getPrototypeOf(vs) === ArrayIteratorProto);
console.log(Object.prototype.toString.call(es), es[Symbol.iterator]() === es);

// length re-read every step; a getter observes each read
var log: string[] = [];
var grow: any = { get length() { log.push("len"); return this.n; }, n: 1, 0: "x", 1: "y" };
var gi: any = Array.prototype.values.call(grow);
console.log(gi.next().value);
grow.n = 2;
console.log(gi.next().value, gi.next().done, log.length);

// holes read undefined, indices past length stop
var sparse: any = { length: 3, 1: "mid" };
console.log(JSON.stringify(Array.from(Array.prototype.values.call(sparse))));
// exhaustion latches: a later growth does not revive
var latch: any = { length: 1, 0: 1 };
var li: any = Array.prototype.values.call(latch);
li.next(); li.next();
latch.length = 5; latch[1] = 2;
console.log(li.next().done);
// a string receiver iterates code units; a Date with bag indices too
console.log(JSON.stringify(Array.from(Array.prototype.values.call("hi"))));
var d: any = new Date(0); d.length = 2; d[0] = 7; d[1] = 8;
console.log(JSON.stringify(Array.from(Array.prototype.entries.call(d))));
// spread / for-of consume it like any array iterator
console.log([...Array.prototype.keys.call({ length: 3 })].join(","));
