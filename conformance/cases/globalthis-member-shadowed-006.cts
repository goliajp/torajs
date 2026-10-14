"use strict";
// strict code: a block function is block scoped, and `globalThis.X`
// after the block reads the builtin
function g() {
  {
    function Math() {
      return 9;
    }
  }
  return globalThis.Math.max(1, 2);
}
console.log(g());
{
  function JSON() {
    return 8;
  }
}
console.log(globalThis.JSON.stringify([2]));
function h() {
  {
    class Number {
      static k = 4;
    }
  }
  return globalThis.Number.k;
}
console.log(h());
