// RC-2b (RFC 20260706-test262-bug-corpus): WeakMap / WeakSet methods
// through any receivers — has/get/set/delete + add ride the
// torajs-weak ptr-keyed kernels; a primitive key reads as absent.
var foo = {};
var bar = {};
var map = new WeakMap();
console.log(map.has(foo));
map.set(foo, bar);
console.log(map.has(foo), map.has(bar));
console.log(map.get(foo) === bar);
map.delete(foo);
console.log(map.has(foo), map.get(foo));
var ws = new WeakSet();
ws.add(foo);
console.log(ws.has(foo), ws.has(bar));
ws.delete(foo);
console.log(ws.has(foo));
console.log(map.has(5), map.delete(5));
