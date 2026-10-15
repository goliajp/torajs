// A class declaration's name resolves to the class, both in the scope
// that declares it and inside its own body, even when an enclosing
// function declares a function of the same name (whose instances have
// no `k`).

function outer() {
  function W() {}
  function inner() {
    class W {
      k = "cls";
      static make() {
        return new W();
      }
    }
    return W.make().k + "/" + new W().k;
  }
  return inner() + "/" + (new (W as any)() as any).k;
}

function inBlock() {
  function V() {}
  let r = "";
  {
    class V {
      k = "cls";
    }
    r = new V().k;
  }
  return r + "/" + (new (V as any)() as any).k;
}

function inCase(n: number) {
  function U() {}
  switch (n) {
    case 1:
      class U {
        k = "cls";
        m() {
          return new U().k;
        }
      }
      return new U().m();
  }
  return (new (U as any)() as any).k;
}

// A binding inside the class body shadows the class's own name again.
class Tw {
  k = "cls";
  n() {
    function Tw() {}
    return (new (Tw as any)() as any).k;
  }
  m() {
    return new Tw().k;
  }
}

console.log(outer());
console.log(inBlock());
console.log(inCase(1), inCase(2));
console.log(new Tw().n(), new Tw().m());
