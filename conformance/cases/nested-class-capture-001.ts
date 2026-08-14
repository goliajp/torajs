// A class declared in a block, reading a binding of that block.
// The class cannot be lifted to the top level (nothing up there
// resolves `a`), so it takes the runtime-value lane instead.
{
  let a = 7;
  class K {
    m(): number {
      return a;
    }
  }
  console.log(new K().m());
}
