// A this-reading static method at a direct spread call site. The
// site the static expanders decline went to the runtime spread lane
// through the mono's relay, which is the plain-call value and hands
// the twin an undefined receiver: `E.self(5, ...arr)` threw on
// `this.k` and `this === E` answered false, while the same call
// without a spread and `E.self.apply(E, [5, ...arr])` both ran. The
// site now routes to the twin with the class as its receiver.
// Length-only and argument-value bodies, an inherited static through
// the subclass, and a this-free static beside them.
class E {
  static k = 7;
  static self(a) { return [this.k, a, arguments.length]; }
  static sv(a) { return [this === E, a, arguments[2]]; }
  static free(a) { return [a, arguments.length]; }
}
class F extends E { static k = 8; }
const arr = [1, 2];
console.log(JSON.stringify(E.self(5, ...arr)), JSON.stringify(E.self(5)));
console.log(JSON.stringify(E.sv(5, ...arr)), JSON.stringify(E.sv(...arr)));
console.log(JSON.stringify(F.self(5, ...arr)), JSON.stringify(F.self(...arr, 9)));
console.log(JSON.stringify(E.free(5, ...arr)), JSON.stringify(E.self.apply(E, [5, ...arr])));
