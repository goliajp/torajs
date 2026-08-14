// `Object.defineProperty(K, …)` on a constructor written as a
// this-using function expression. The target argument never invokes K
// -- §20.1.2.4 rejects a non-object, takes a key, builds a descriptor
// and defines -- so the position is receiver-safe for the same reason
// a member's object is. Before that, one such call took the whole
// binding off the promotion lane and the constructor's own `this`
// answered `unknown identifier __this`.
//
// This is how a class spells a static member once it is lowered to the
// ES5 constructor pattern AND has to keep §15.7.14's non-enumerability,
// which an assignment cannot give it.
//
// The observations happen from OUTSIDE, through the returned value: a
// `for…in` source and an `Object.getOwnPropertyNames` argument are not
// themselves admitted positions, so asking the promoted binding
// directly would take it off the lane again -- and going through the
// escaped alias is the shape the class lane actually produces anyway.
function build(): any {
  const K: any = function (p: number) {
    (this as any).x = p;
  };
  Object.defineProperty(K, "base", { value: 40, writable: true, configurable: true });
  Object.defineProperty(K, "t", {
    value: function () {
      return (this as any).base;
    },
    writable: true,
    configurable: true,
  });
  // A static method whose body says `this` reaches the function itself,
  // not a property bag: it calls the sibling static through it.
  Object.defineProperty(K, "s", {
    value: function () {
      return (this as any).t() + 2;
    },
    writable: true,
    configurable: true,
  });
  Object.defineProperty(K, "half", {
    get: function () {
      return (this as any).base / 2;
    },
    configurable: true,
  });
  // The plural form takes the same target position.
  Object.defineProperties(K, {
    twice: {
      value: function () {
        return (this as any).base * 2;
      },
      writable: true,
      configurable: true,
    },
  });
  // The prototype half still works alongside it.
  Object.defineProperty(K.prototype, "plus", {
    value: function (n: number) {
      return (this as any).x + n;
    },
    writable: true,
    configurable: true,
  });
  return K;
}

const A: any = build();

console.log(new A(1).x, new A(1).plus(9));
console.log(A.t(), A.s(), A.half, A.twice());
// An explicit receiver still wins over the function itself.
console.log(A.s.call({ t: function () { return 100 } }));

// Nothing defined this way is enumerable, which is the point.
const seen: string[] = [];
for (const k in A) seen.push(k);
console.log("[" + seen.join("|") + "]");
console.log(Object.getOwnPropertyNames(A).indexOf("s") >= 0);
