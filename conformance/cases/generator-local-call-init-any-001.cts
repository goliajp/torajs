// A generator's locals become fields on the state-machine object, and
// a field whose initializer the lift cannot type fell back to
// `number` — right for the loop counter that motivated it, wrong for
// everything else. Two shapes already carried a carve-out (a
// destructuring source temp, a free Ident); a CALL is the third, and
// by the same argument: a top-level function answers through the
// signature table and a local closure or class generator method
// through the call-result sniff, so a call that still reaches the
// fallback is a builtin whose result this pass has no static type
// for. `JSON.parse` was pinned to Number and every read off it
// refused, while the same line outside a generator is fine.
function* parsedObject() {
  var b = JSON.parse('{"a":1,"b":[2,3]}');
  yield typeof b;
  yield b.a;
  yield b.b.length;
}

function* parsedArray() {
  var xs = JSON.parse("[4,5,6]");
  yield xs.length;
  yield xs[2];
}

function* stillTyped() {
  // A call the sniff CAN type keeps its type — this one is a local
  // closure, and the field stays what it always was.
  var f = function () {
    return 7;
  };
  var n = f();
  yield n + 1;
}

function drive(g, n) {
  var it = g();
  var out = [];
  for (var i = 0; i < n; i++) out.push(it.next().value);
  return out.join(",");
}

console.log("object", drive(parsedObject, 3));
console.log("array ", drive(parsedArray, 2));
console.log("typed ", drive(stillTyped, 1));
