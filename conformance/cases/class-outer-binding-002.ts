// The scope §14.2.3 names is the one the class was WRITTEN in, and
// for a class the nested-class hoist renames on collision that scope
// is not the program. Its outer binding therefore stays behind in the
// container, where the reference rewrite has just renamed everything
// to match — otherwise two blocks' classes fight over one spelling.
{
  class C {
    static tag = "first";
  }
  console.log(C.tag);
}
{
  class C {
    field: any = () => C;
  }
  console.log(new C().field() === C);
  const kept: any = C;
  C = null as any;
  console.log(new kept().field() === kept, C);
}

// A write inside a block whose class the ES5-capturing lane claims
// (`__cc<N>_<C>`) still lands on an immutable binding — a third lane
// with its own account, recorded, not this one.
