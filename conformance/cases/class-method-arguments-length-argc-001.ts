// a class method that reads only `arguments.length` answers the count
// each call wrote, whatever its optional and defaulted parameters pad
class E {
  m(a?: number, b?: number) {
    return arguments.length;
  }
  d(a: number, b: number = 5) {
    return arguments.length + ":" + b;
  }
}
const e = new E();
console.log(e.m(), e.m(1), e.m(1, 2));
console.log(e.d(1), e.d(1, 2), e.d(1, undefined));
const r: any = e;
console.log(r.m(1, 2, 3), r.m(), r.d(7, 8));

// inherited without an override: the subclass instance runs E's body
class F extends E {
  other() {
    return 1;
  }
}
const f = new F();
console.log(f.m(), f.m(4), f.d(1), f.d(1, 2), (f as any).m(1, 2, 3, 4));

// the method read off as a value and called
const mv = e.m;
console.log(mv.call(e, 1, 2, 3), mv.call(e));
const dv: any = e.d;
console.log(dv.call(e, 9));

// a spread call keeps a method off the direct argc lane; the runtime
// lane still answers the count it spread
class S {
  s(a: number) {
    return arguments.length;
  }
}
const sp = new S();
const xs = [1, 2, 3];
console.log(sp.s(...xs), sp.s(...([] as number[])));
