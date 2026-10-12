// §B.3.5 lets a block function under a catch clause share the catch
// parameter's name, and §B.3.3.1 writes the function into the var scope
// of the enclosing function (`varEnv.SetMutableBinding`), not into the
// parameter. Inside the catch block `f` is still the caught value; after
// the try, the function's var binding holds the function. In an async
// body tr wrote the var where the parameter caught it, so the catch
// block saw the function and the var stayed undefined.
//
// bun is not an oracle here (it reads every file as strict code, with
// no var binding). The `.expected` holds the spec's answers, which are
// node's.

async function a() {
  await 0;
  try { throw 1; } catch (f) {
    { function f() { return 7; } }
    console.log(typeof f, f);
  }
  console.log(typeof f, f());
}

function* g() {
  yield 0;
  let x = 5;
  let inside = "";
  try { throw 2; } catch (f) {
    { function f() { return x; } }
    inside = typeof f;
  }
  const after = typeof f;
  const r = f();
  yield inside;
  yield after;
  yield r;
}

a().then(() => {
  for (const v of g()) console.log(v);
});
