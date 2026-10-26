// The boxed entry boxes the literal's function's own answer by its
// declared return, here an object, through an interface whose class row
// returns an any value of another kind.
const raw: any = JSON.parse('{"o":"q"}');
interface K {
  m(): { k: number };
  n: number;
}
class T implements K {
  v: any;
  n = 1;
  constructor(v: any) {
    this.v = v;
  }
  m(): { k: number } {
    return this.v.o;
  }
}
function call(o: K) {
  return o.m();
}
function h(): { k: number } {
  return { k: 3 };
}
const o: K = { m: h, n: 1 };
console.log(JSON.stringify(call(o)), call(new T(raw)), h().k, call({ m: h, n: 2 }).k);
