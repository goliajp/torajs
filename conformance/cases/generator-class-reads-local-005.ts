// A `var` in a `for` head is a declaration of the enclosing function
// (§14.7.4.2 VarDeclaredNames), visible after the loop. Inside a class
// member or an arrow in a generator body it shadows the generator's
// local of the same name, before, during and after the loop.

function* member() {
  let i = 100;
  class C {
    m() {
      for (var i = 0; i < 3; i++) {}
      return i;
    }
  }
  yield new C().m();
  yield i;
}
for (const v of member()) console.log(v);

function* arrow() {
  let i = 100;
  const h = () => {
    for (var i = 0; i < 4; i++) {}
    return i;
  };
  yield h();
  yield i;
}
for (const v of arrow()) console.log(v);

function* arrowList() {
  let i = 100;
  let j = 7;
  const h = () => {
    for (var i = 0, j = 5; i < 2; i++) {}
    return i + j;
  };
  yield h();
  yield i + j;
}
for (const v of arrowList()) console.log(v);

function* memberForOf() {
  let x = 9;
  class C {
    m() {
      for (var x of [1, 2, 3]) {
      }
      return x;
    }
  }
  yield new C().m();
  yield x;
}
for (const v of memberForOf()) console.log(v);

function* readBeforeLoop() {
  let n = 50;
  class C {
    m() {
      const before = n;
      for (var n = 0; n < 2; n++) {}
      return String(before) + " " + n;
    }
  }
  yield new C().m();
  yield n;
}
for (const v of readBeforeLoop()) console.log(v);
