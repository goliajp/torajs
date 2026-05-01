// Adapted from test262: language/statements/class/* — classes can declare
// `T[]` fields and grow them through methods. tr's desugar hoists the
// implicit default into a typed prelude let so the factory's object
// literal sees a fully-typed ident; combined with `this.xs.push(v)`
// (struct-field push), classes own and mutate their array state.
//
// The constructor explicitly initializes each array via a typed local —
// this stays bun-compatible (where class-field declarations don't
// auto-initialize) AND exercises tr's same hoisting + struct-field push
// path.
class Bag {
  xs: number[];
  count: number;
  constructor() {
    let init_xs: number[] = [];
    this.xs = init_xs;
    this.count = 0;
  }
  add(v: number): void {
    this.xs.push(v);
    this.count = this.count + 1;
  }
  total(): number {
    let s: number = 0;
    for (let v of this.xs) { s += v; }
    return s;
  }
}

class Labels {
  names: string[];
  tag: string;
  constructor(t: string) {
    let init_names: string[] = [];
    this.names = init_names;
    this.tag = t;
  }
  pushLabel(n: string): void {
    this.names.push(n);
  }
}

function check(): number {
  let b = new Bag();
  if (b.count !== 0) { throw "#1: count default"; }
  if (b.xs.length !== 0) { throw "#2: xs default"; }
  b.add(7);
  b.add(8);
  b.add(5);
  if (b.count !== 3) { throw "#3: count after add"; }
  if (b.xs.length !== 3) { throw "#4: xs len"; }
  if (b.xs[0] !== 7) { throw "#5"; }
  if (b.xs[2] !== 5) { throw "#6"; }
  if (b.total() !== 20) { throw "#7"; }

  // string[] field — different element type, same hoisting machinery.
  let l = new Labels("group-a");
  l.pushLabel("alpha");
  l.pushLabel("beta");
  if (l.tag !== "group-a") { throw "#8"; }
  if (l.names.length !== 2) { throw "#9"; }
  if (l.names[1] !== "beta") { throw "#10"; }
  return 0;
}
console.log(check());
