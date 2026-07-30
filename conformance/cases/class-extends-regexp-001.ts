// class C extends RegExp — exotic-backed regexp instances (RFC
// 20260730 blade 2). The instance is a REAL RegExp cell (test/exec/
// source/flags ride the existing arms); super(pattern) recompiles
// the minted instance per §22.2.3.1; class identity rides
// FLAG_SUBCLASSED + the blade-0 side table.

// 1. explicit ctor forwarding the pattern
class MyRe extends RegExp {
  constructor(p: any) {
    super(p);
  }
}
const r = new MyRe("ab+c");
console.log(r instanceof RegExp, r instanceof MyRe);
console.log(Object.getPrototypeOf(r) === MyRe.prototype);
console.log(r.test("xabbc"), r.test("xac"));
console.log(r.source);

// 2. RegExp pattern argument copies source + flags (§22.2.3.1 step 5)
const seed = /d[eE]f/;
const c = new MyRe(seed);
console.log(c.test("xdEf"), c.test("xdf"), c.source);

// 3. default ctor — the mint's default is the empty pattern
class EmptyRe extends RegExp {}
const e = new EmptyRe();
console.log(e.test(""), e.test("anything"), e instanceof EmptyRe);

// 4. class methods over the exotic receiver
class Tagged extends RegExp {
  constructor(p: any) {
    super(p);
  }
  label(): string {
    return "T";
  }
}
const t = new Tagged("x\\d+");
console.log(t.label(), t.test("x42"), t.test("xx"));

// 5. plain regexes keep their answers
console.log(/abc/ instanceof MyRe, /abc/.test("xabc"));
