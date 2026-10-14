// A static field with no initializer is defined with the value undefined,
// for a class written inside a function as much as at the top level.

class Top {
  static x;
}
console.log(Top.x);

function f() {
  class Inner {
    static y;
    static z = undefined;
    static w = 5;
  }
  console.log(Inner.y, Inner.z, Inner.w, "y" in Inner);
}
f();

(function () {
  class Named {
    static accessor;
  }
  console.log(Named.accessor);
})();
