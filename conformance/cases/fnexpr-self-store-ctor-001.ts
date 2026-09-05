// A constructor function that stores ITSELF on the instance. The key
// `self` is declared by no nominal type in the program, so the store
// lands in the instance's expando dict and the read comes back a NaN
// box — every path out of it honours the receiver channel, which is
// what lets the binding's receiver promotion stand. Before rotation
// 589 the self-read was an unadmitted use of the binding and the
// program died on `fnexpr this in unclaimed receiver position`, while
// the same constructor without the self-read compiled.
var Maker = function (n) {
  this.n = n;
  this.self = Maker;
};

var a = new Maker(3);
console.log(a.n, a.self === Maker);

// the stored copy still constructs, and still receives its own `this`
var b = new a.self(4);
console.log(b.n, b.self === Maker);

// and still takes a receiver when called as somebody else's method.
// The host is built by expando store, not by an object literal: a
// literal spelling `self` would make that key nominal program-wide
// and the census would refuse the store above — the coarseness the
// module doc names.
var host = {};
host.make = a.self;
host.make(9);
console.log(host.n, host.self === Maker);
