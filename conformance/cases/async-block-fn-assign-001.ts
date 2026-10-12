// A function declaration's binding is mutable (§10.2.11 step 36,
// CreateMutableBinding), in a block as anywhere. In an async or
// generator body tr turns a declaration that reads its surroundings
// into a value binding, and made it a constant: assigning the name was
// refused as "cannot assign to const".

function* gen() {
  yield 0;
  let x = 1;
  {
    function h() { return x; }
    yield h();
    h = 0 as any;
    yield h;
  }
  function top() { return x + 1; }
  yield top();
  top = (() => 7) as any;
  yield top();
}
for (const v of gen()) console.log(v);

async function a() {
  let x = 2;
  await 0;
  {
    function q() { return x; }
    console.log(q());
    q = 3 as any;
    console.log(q);
  }
  let n = 0;
  function bump() { return ++n; }
  bump = (() => 40 + n) as any;
  console.log(bump());
}
a();
