// A `var` declared inside a top-level block is a global (§8.2 var
// scoping), so a generator expression that writes it captures
// nothing — the hoist must read block-nested vars as globals, not
// as reassigned captures (test262 unscopables-with family).
let count = 0;
var v = 1;
globalThis[Symbol.unscopables] = { v: true };
{
  count++;
  var callCount = 0;
  var ref;
  ref = function* (x) {
    count++;
    with (globalThis) {
      count++;
      console.log("v1", v);
    }
    count++;
    var v = x;
    with (globalThis) {
      count++;
      console.log("v2", v);
      v = 20;
    }
    console.log("v3", v, globalThis.v);
    callCount = callCount + 1;
  };
  ref(10).next();
  console.log("cc", callCount);
  count++;
}
console.log("count", count);

// declaration form inside a block, same var
{
  var dc = 0;
  function* decl(x) {
    dc = dc + 1;
    yield x;
  }
  console.log(decl(7).next().value, "dc", dc);
}

// async generator expression, same var — async section last
{
  var ac = 0;
  var aref;
  aref = async function* g(x) {
    ac = ac + 1;
    yield x;
  };
  aref(3).next().then((r) => {
    console.log("ac", ac, r.value, r.done);
  });
}
