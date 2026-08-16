// Reading an inherited Object.prototype method as a VALUE, not
// calling it. The per-tag readable-surface table listed each family's
// own methods and had to be told about the inherited pair one family
// at a time; Map / Set / Promise never were, so `typeof m.toString`
// answered undefined while `m.toString()` answered "[object Map]".
var m = new Map([["a", 1]]);
var s = new Set([1]);
var p = Promise.resolve(1);
var sym = Symbol("d");
var b = 3n;

console.log("map", typeof m.toString, typeof m.toLocaleString, typeof m.valueOf);
console.log("set", typeof s.toString, typeof s.has);
console.log("promise", typeof p.toString, typeof p.then, typeof p.catch, typeof p.finally);
console.log("symbol", typeof sym.toString, typeof sym.valueOf);
console.log("bigint", typeof b.toString, typeof b.toLocaleString);

// the value read is the real method — borrowing it across receivers
// runs the §20.1.3.6 badge against whoever it is called on
const f = m.toString;
console.log("borrowed", f.call(m), f.call(s), f.call(p));
console.log("direct", m.toLocaleString(), s.toString());

// families that own these names keep their own bodies
var d = new Date(0);
var a = [1, 2];
console.log("own", typeof d.toString, typeof a.toString, a.toString(), b.toString(2));

var o = { x: 1 } as any;
console.log("object", typeof o.toString, o.toString(), typeof o.__defineGetter__);
