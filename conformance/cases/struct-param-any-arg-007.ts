// A class method with a struct-typed parameter now has an all-any twin
// for the arguments that do not fit the struct. The twin's body is
// still inside the class scope, so a write to the class's own name in
// it is the §15.7.14 runtime TypeError, not a compile-time refusal,
// whether the call came in typed or through `any`.
type P = { x: number; y: number };
let sideEffects = 0;
function rhs(): any {
  sideEffects++;
  return 1;
}

class C {
  static v = 7;
  tag(p: P): string {
    try {
      C = rhs();
    } catch (e: any) {
      return "method " + e.constructor.name + " x=" + p.x;
    }
    return "unreachable";
  }
  static s(p: P): string {
    try {
      C = rhs();
    } catch (e: any) {
      return "static " + e.constructor.name + " y=" + p.y;
    }
    return "unreachable";
  }
}

const c = new C();
console.log(c.tag({ x: 1, y: 2 }));
console.log(C.s({ x: 3, y: 4 }));
const m: any = c.tag.bind(c);
const s: any = C.s;
console.log(m([5, 6]));
console.log(m("str"));
console.log(s(new Proxy({ x: 8, y: 9 }, {})));
console.log(s(3));
console.log("rhs ran", sideEffects);
console.log("C survived", C.v, typeof C);
