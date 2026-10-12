// A function declaration's `name` is the name it was declared with
// (§15.2.5 InstantiateOrdinaryFunctionObject → SetFunctionName). In an
// async or generator body tr turned a declaration that reads its
// surroundings into a value binding, whose function came out nameless,
// and an Annex B block function took the name of its internal block
// binding.
//
// bun is not an oracle here: it answers "" for a block function in a
// generator body. The `.expected` holds the spec's answers, which are
// node's.

{
  function plain() {}
  console.log(plain.name);
}

function* g() {
  yield 0;
  let x = 1;
  { function h() { return x; } yield h.name; }
  { function lone() {} yield lone.name; }
  function m() { return x; }
  yield m.name;
}
for (const v of g()) console.log(v);

async function a() {
  let x = 1;
  await 0;
  { function h() { return x; } console.log(h.name); }
  function m() { return x; }
  console.log(m.name);
}
a();
