// A computed name on an ACCESSOR of a captured-scope class, instance
// and static. Both were declined while `Object.defineProperty` could
// not take the class binding as its target: an accessor has no
// assignment spelling at all, so a computed one had two reasons to be
// turned down and neither is left. The key is the same
// `__ccmk_<C>_<n>` binding a computed method already reads, and the
// descriptor does not care whether the name was written out or
// evaluated.
//
// A getter and a setter sharing one key are two MethodDefinitions, so
// §15.7.14 evaluates that key TWICE and emits two defines. The second
// keeps the first half (§10.1.6.3 step 4), which is why the pair below
// ends up on one property rather than the setter erasing the getter.
function make(base: number): any {
  const pair = "span" + base;
  const stat = "origin" + base;
  let keyEvals = 0;
  const counted = function (): string {
    keyEvals = keyEvals + 1;
    return "counted";
  };
  class Range {
    n: number;
    constructor(start: number) {
      this.n = start + base;
    }
    get [pair]() {
      return this.n * 2;
    }
    set [pair](v: number) {
      this.n = v + base;
    }
    get [counted()]() {
      return keyEvals;
    }
    static get [stat]() {
      return base * 5;
    }
    // The count is read back through the class rather than returned
    // alongside it: an array literal element is a container store, and
    // that is one of the positions that would take the binding off the
    // promotion lane -- the test harness is a use of the binding too.
    static get evals() {
      return keyEvals;
    }
  }
  return Range;
}

const Tens: any = make(10);
const Fours: any = make(4);

const t: any = new Tens(1);
console.log(t.n, t["span10"]);
t["span10"] = 100;
console.log(t.n, t["span10"]);

// The key ran once, at class definition -- not per construction.
new Tens(1);
new Tens(2);
console.log(Tens.evals, t["counted"]);

// Static side, and two factory calls keep their keys apart.
console.log(Tens["origin10"], Fours["origin4"]);

// Accessors a class declares are non-enumerable either way.
const statics: string[] = [];
for (const k in Tens) statics.push(k);
const own: string[] = [];
for (const k in t) own.push(k);
console.log("[" + statics.join("|") + "]", "[" + own.join("|") + "]");
