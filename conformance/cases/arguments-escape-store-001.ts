// RFC 20260808 escape-store profile — an arguments-touching fn-expr
// whose binding is STORED into a boxed-face position (the fnexpr-this
// B2 store roots) joins the argv face instead of killing the chain:
// every consumer of that slot (species construct, any-lane member
// call) enters the boxed dual entry with real argc/argv. The
// this+arguments combination exercises the recv-slot parameter order
// (`__this` must sit AFTER the injected argc/argv slots — the
// pre-fix insert put it first and the adapter read boxed values as
// the argv pointer).
var thisValue: any, args: any, result: any;
var callCount = 0;
var instance: any = [];
var Ctor: any = function () {
  callCount += 1;
  thisValue = this;
  args = arguments;
  return instance;
};
var a: any = [];
a.constructor = {};
a.constructor[Symbol.species] = Ctor;
result = a.concat();
console.log(callCount, result === instance, args.length, args[0]);
var args2: any;
var g: any = function () {
  args2 = arguments;
};
var o: any = {};
o.k = g;
o.k(7, 8, 9);
console.log(args2.length, args2[1]);
var args4: any, tv: any;
var C4: any = function () {
  tv = this;
  args4 = arguments;
};
var a4: any = [];
a4.constructor = {};
a4.constructor[Symbol.species] = C4;
var f4: any = a4.constructor[Symbol.species];
f4(4, 5);
console.log(args4.length, args4[0], tv === undefined);
