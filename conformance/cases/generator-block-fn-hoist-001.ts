// A function declared in a block is initialized on block entry
// (§14.2.3 BlockDeclarationInstantiation), so a call written above its
// text works. In a generator body the call sat in one state of the
// machine and the declaration in none it could reach: `q is not
// defined`. Outside the block the name is not bound (strict code).

function* g() {
  {
    yield q();
    function q() { return 3; }
  }
  {
    {
      yield r(2);
      function r(n: number): number { return n <= 0 ? 0 : n + r(n - 1); }
    }
  }
  yield typeof q;
}
for (const v of g()) console.log(v);

async function a() {
  {
    await 0;
    console.log(s());
    function s() { return "s"; }
  }
}
a();
