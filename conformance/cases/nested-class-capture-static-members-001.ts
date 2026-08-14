// Static members of a captured-scope class: non-enumerable (§15.7.14),
// accessors, and computed names. All three were queued behind one
// thing -- installing a static member means `Object.defineProperty(K,
// …)`, which passes the class BINDING as an argument, and until that
// position became receiver-safe doing so took the constructor's `this`
// off the promotion lane. So statics were installed by assignment and
// stayed wrongly enumerable, while a static accessor or a computed
// static name -- neither of which an assignment can even spell -- was
// declined outright.
//
// The enumerability is observed through the RETURNED class, not the
// binding inside: a `for…in` source is hoisted into its own alias
// binding by the parser, and an alias init is the one shape the
// receiver-safe list cannot admit.
function make(base: number): any {
  const key = "scaled" + base;
  class Box {
    n: number;
    constructor(start: number) {
      this.n = start + base;
    }
    grow() {
      return this.n + base;
    }
    // A static method may say `this`, and it means the class.
    static origin() {
      return base;
    }
    static twice() {
      return this.origin() * 2;
    }
    static get half() {
      return base / 2;
    }
    static set half(v: number) {
      this.mark = v * 100;
    }
    static [key]() {
      return base * 10;
    }
  }
  return Box;
}

const Tens: any = make(10);
const Fours: any = make(4);

console.log(new Tens(1).n, new Tens(1).grow());
console.log(Tens.origin(), Tens.twice(), Tens.half);
Tens.half = 3;
console.log(Tens.mark);

// A computed static name, and two calls of the factory keep theirs apart.
console.log(Tens["scaled10"](), Fours["scaled4"]());

// Nothing a class DECLARES is enumerable -- statics included, which is
// what this whole knife was about. `mark` is in the answer precisely
// because it was not declared: the setter above created it by ordinary
// assignment, and that makes an enumerable property.
const statics: string[] = [];
for (const k in Tens) statics.push(k);
const own: string[] = [];
for (const k in new Tens(1)) own.push(k);
console.log("[" + statics.join("|") + "]", "[" + own.join("|") + "]");
console.log(Object.getOwnPropertyNames(Tens).indexOf("origin") >= 0);
