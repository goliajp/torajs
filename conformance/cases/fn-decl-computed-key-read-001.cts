// A computed key read off a function declaration's name reads a property
// of the function object, inherited from Function.prototype here
// (§20.2.3.6 Function.prototype[@@hasInstance]).

function z() {}
const k = Symbol.hasInstance;
console.log(typeof z[Symbol.hasInstance]);
console.log(z[Symbol.hasInstance](new z()), z[k]({}));

// A sloppy block function's references inside its block name its block
// binding, which is the same function object.
{
  function zz() {}
  const inst = new zz();
  console.log(inst instanceof zz, zz[Symbol.hasInstance](inst));
}

function inner() {
  {
    function w() {}
    return w[Symbol.hasInstance](new w());
  }
}
console.log(inner());
