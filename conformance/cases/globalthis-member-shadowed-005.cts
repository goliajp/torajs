// A class declared in a block is visible only in that block, so
// `globalThis.X` past it reads the builtin; the same holds when the
// block is at the top level, and for a function declared in a block.
function g() {
  {
    class Math {
      static max(a, b) {
        return 7;
      }
    }
  }
  return globalThis.Math.max(1, 2);
}
console.log(g());
function h() {
  if (true) {
    class Number {
      static k = 4;
    }
  }
  return globalThis.Number.k;
}
console.log(h());
function r() {
  {
    class Reflect {
      static k = 3;
    }
  }
  return typeof globalThis.Reflect.ownKeys;
}
console.log(r());
{
  class JSON {
    static stringify(v) {
      return "mine";
    }
  }
}
console.log(globalThis.JSON.stringify([1]));
{
  class WeakMap {
    static k = 2;
  }
}
console.log(globalThis.WeakMap.k);
function bf() {
  {
    function Math() {
      return 9;
    }
  }
  return globalThis.Math.max(1, 2);
}
console.log(bf());
function bf2() {
  var n = globalThis.Reflect.ownKeys({ a: 1 }).length;
  {
    function Reflect() {}
  }
  return n;
}
console.log(bf2());
