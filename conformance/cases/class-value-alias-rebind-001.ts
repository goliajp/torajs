// A class-value alias is a parse-order note that one binding holds a
// particular class expression, so `C.m()` can go straight to that
// class's statics. It is not scoped, so every later binding of the
// spelling has to drop it — `let` and assignment already did, a class
// declaration did not, and the stale note routed the read to the
// class expression from the block before.
{
  let C: any = class Inner {};
}
{
  class C {
    static t = "x";
  }
  console.log(C.t);
}

// The same note reached instance state and static methods.
{
  let D: any = class Inner {};
}
{
  class D {
    f = 5;
    static m() {
      return "M";
    }
  }
  console.log(new D().f, D.m());
}

// The inner name binding still answers the class the block declares,
// not the one the alias remembered.
{
  let E: any = class Inner {};
}
{
  class E {
    field: any = () => E;
  }
  console.log(new E().field() === E);
}

// Dropping the note must not cost a live alias its own reads.
const F = class {
  static t = "f";
};
console.log(F.t, new F() instanceof F);
const G = class Named {
  static t = "g";
  static self() {
    return Named;
  }
};
console.log(G.name, G.self() === G, G.t);
