// A class declaration's outer binding is mutable (§14.2.3), and a
// class that lives in a nested scope takes the ES5 lane, which mints
// one binding for both readings. That binding was immutable, so a
// legal assignment was refused outright.
{
  class C {
    static t = "a";
  }
  console.log(C.t);
  C = 9 as any;
  console.log(C);
}

// Reassigning through the outer binding is what the later reads see.
function f() {
  class D {
    static tag = "d";
  }
  const keep: any = D;
  D = null as any;
  return [D, keep.tag];
}
console.log(f());

// A class that DOES read its own name keeps the two bindings apart:
// the body must go on seeing the class whatever the outer binding is
// set to, so this lane keeps its single binding immutable and says so
// rather than letting the write leak in.
{
  class E {
    field: any = () => E;
  }
  console.log(new E().field() === E);
}
function g() {
  class H {
    static self() {
      return H;
    }
  }
  return H.self() === H;
}
console.log(g());
