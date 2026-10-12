// nested generator declarations: `yield*` into a non-capturing sibling
// from a capturing one, destructured params on the lifted lane, a call
// above the declaration, a generator nested in a capturing one, and
// same-named generators in different functions with different params
function delegate(k: number) {
  function* src() { yield 1; yield 2 }
  function* user() { yield k; yield* src(); yield k + 1 }
  return [...user()].join(",");
}

function destr() {
  function* g([x, y]: number[]) { yield x; yield y }
  function* h({ z }: { z: number }) { yield z }
  return [...g([1, 2])].join(",") + "|" + [...h({ z: 3 })].join(",");
}

function hoisted(k: number) {
  const first = [...g()].join(",");
  function* g() { yield k; yield k * 2 }
  return first;
}

function deep(k: number) {
  function* g() {
    function* h() { yield k }
    yield* h();
    yield k + 1;
  }
  return [...g()].join(",");
}

function destrCapture(k: number) {
  function* g([x]: number[]) { yield x + k }
  return [...g([1])].join(",");
}

console.log(delegate(3));
console.log(destr());
console.log(hoisted(4));
console.log(deep(5));
console.log(destrCapture(10));
