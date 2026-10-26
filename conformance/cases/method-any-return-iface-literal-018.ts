// Object literals of an interface whose method returns an any value, in
// each place a literal can be handed over: an argument of a function
// call, of a method call and of a constructor, the return of a function
// declared to return the interface, an array element, a binding with
// no annotation. Each holds a function also called directly.
const raw: any = JSON.parse('{"x":1.5}');
interface J {
  m(): number;
  n: number;
}
class S implements J {
  v: any;
  n = 1;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
function call(o: J) {
  return o.m() + o.n;
}
class Box {
  o: J;
  constructor(o: J) {
    this.o = o;
  }
  run(o: J) {
    return o.m() * 2;
  }
}
function g(): number {
  return 3.25;
}
function mk(): J {
  return { m: g, n: 4 };
}
const xs: J[] = [{ m: g, n: 5 }];
const bare = { m: g, n: 6 };
console.log(call({ m: g, n: 1 }), new Box({ m: g, n: 2 }).run({ m: g, n: 3 }), call(new Box({ m: g, n: 2 }).o));
console.log(call(mk()), call(xs[0]), call(bare), call(new S(raw)), g());
