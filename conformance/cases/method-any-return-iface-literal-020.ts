// Object literals of an interface whose method returns an any value,
// handed straight to a function's parameter and to a method's, and
// returned from a function declared to return the interface. Each holds
// a function also called directly; strings come back as they are.
const raw: any = JSON.parse('{"x":1.5,"s":"ab"}');
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
class T implements J {
  v: any;
  n = 2;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.s;
  }
}
function call(o: J) {
  return o.m() + o.n;
}
class Runner {
  run(o: J) {
    return o.m() + o.n * 2;
  }
}
function g(): number {
  return 3.25;
}
const w = (): number => 0.5;
function mk(): J {
  return { m: w, n: 4 };
}
console.log(call({ m: g, n: 1 }), new Runner().run({ m: g, n: 3 }), call(mk()));
console.log(call(new S(raw)), call(new T(raw)), new Runner().run(new T(raw)), g(), w());
