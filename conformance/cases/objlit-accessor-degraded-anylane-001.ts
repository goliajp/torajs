// an accessor-bearing object literal whose binding a LATER statement
// pushes onto the dynobj lane: Object.defineProperty on it, a member
// delete, or a computed-key write. Each converts the runtime cell,
// so the accessor face has to take the any receiver rather than the
// nominal __ObjLit_n stamp its own site would have earned.

// defineProperty receiver
var byDefine = { n: 1, get twice() { return this.n * 2; } };
Object.defineProperty(byDefine, "extra", { value: 9, enumerable: true });
console.log(byDefine.twice, byDefine.extra);

// member delete
var byDelete = { x: 5, n: 3, get twice() { return this.n * 2; } };
delete byDelete.x;
console.log(byDelete.twice, byDelete.x);

// computed-key write with a symbol
var bySymbol = { n: 7, get twice() { return this.n * 2; } };
bySymbol[Symbol.iterator] = function () { return 1; };
console.log(bySymbol.twice, typeof bySymbol[Symbol.iterator]);

// expando write under a name the literal does not spell
var byExpando = { n: 4, get twice() { return this.n * 2; } };
byExpando.later = byExpando.twice + 1;
console.log(byExpando.twice, byExpando.later);

// a this-free accessor keeps working through the same lane
var readGets = 0;
var thisFree = { get probe() { readGets += 1; return "hit"; } };
delete thisFree.absent;
console.log(thisFree.probe, readGets);

// setter face on a degraded binding
var stored = "";
var withSetter = { n: 2, set tag(v) { stored = v + this.n; } };
Object.defineProperty(withSetter, "pinned", { value: true });
withSetter.tag = "t";
console.log(stored, withSetter.pinned);

// the accessor still reports as an accessor property
var d = Object.getOwnPropertyDescriptor(byDefine, "twice");
console.log(typeof d.get, d.set, d.enumerable, d.configurable);
