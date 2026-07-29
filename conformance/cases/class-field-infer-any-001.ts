// An untyped class field whose initializer is not a scalar literal
// used to REJECT the program: the parser inferred the slot type from
// the initializer's shape and knew only Number / String / Boolean.
// Real inference needs the initializer's TYPE, which only the checker
// has — the parser can read shape and nothing more. Refusing the rest
// bought no safety (the initializer is parsed and checked either way;
// only the SLOT widens) while costing every field initialized by a
// call, an array, an object, or null. Those now take `any`, the same
// slot an explicit `: any` annotation had always given them.
function mk(): number {
  return 5;
}

class C {
  // the three the parser can still narrow
  num = 7;
  str = "lit";
  bool = true;
  // and the shapes that used to reject
  arr = [1, 2, 3];
  obj = { k: "v" };
  nil = null;
  undef = undefined;
  call = mk();
  fn = function () {
    return 11;
  };
  arrow = (x: any) => x + 1;
  built = new Date(0);
  static st = mk();
}

const c: any = new C();
console.log("narrowed", c.num, c.str, c.bool);
console.log("widened", c.arr[2], c.obj.k, c.nil, c.undef, c.call);
console.log("callable", c.fn(), c.arrow(1));
console.log("builtin", typeof c.built, "static", C.st);
console.log("array-identity", Array.isArray(c.arr), typeof c.arr);

// A field initializer may read an earlier field through `this`.
class D {
  x = 1;
  y = this.x + 1;
}
const d: any = new D();
console.log("this-ref", d.x, d.y);
