// an accessor whose body never says `this` is given the receiver
// anyway (an accessor always takes one), and which LANE the literal
// lowers on is not decidable where that stamp is minted: a call
// argument the checker ends up typing any reaches the dynobj lane
// through routes no syntactic predicate sees. Such a face therefore
// takes the receiver-first any shape, which both lanes can serve.

var reads = 0;

// argument of a method call whose receiver is a call result
class C {
  take({ ...rest }) { return rest.v; }
  takeOne(o) { return o.v; }
}
console.log(new C().take({ get v() { reads += 1; return 2; } }));
console.log(new C().takeOne({ get v() { reads += 1; return 3; } }));

// super-call argument
class Parent {
  seen: number;
  constructor(o) { this.seen = o.c; }
}
class Child extends Parent {
  constructor() { super({ get c() { reads += 1; return 5; } }); }
}
console.log(new Child().seen);

// the plain struct lane still reads the same face
var direct = { get v() { reads += 1; return 7; }, w: 1 };
console.log(direct.v, direct.w);

// setter faces take the same shape
var stored = -1;
function sink(o: any) { o.s = 11; }
sink({ set s(v) { stored = v; } });
console.log(stored);

var localSet = { set s(v) { stored = v + 1; } };
localSet.s = 20;
console.log(stored);

// descriptor shape survives on both lanes
var d1 = Object.getOwnPropertyDescriptor(direct, "v");
console.log(typeof d1.get, d1.set, d1.enumerable, d1.configurable);

console.log(reads);
