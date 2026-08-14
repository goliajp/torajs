// 406-02 — a capturing class with a COMPUTED static field routes
// through the ES5 lane (it used to decline, and before blade 6 the
// splice-to-top path happened to run it — the 4-case sweep
// regression). The key rides the same __ccmk binding a computed
// method reads; the store is defineProperty with CreateDataProperty's
// attributes; ownership is gated on a program-unique class name.

// numeric key, read back through the index face on the class value
{
  let k = 5;
  class C0 {
    static [k] = "s";
  }
  console.log((C0 as any)[5]);
}

// string key, member read
{
  let k = "kk";
  class C1 {
    static [k] = "s1";
  }
  console.log((C1 as any).kk);
}

// initializer that says this reads the class object
{
  let k = "m";
  class C3 {
    static base = 7;
    static [k] = (this as any).base + 1;
  }
  console.log((C3 as any).m);
}

// the test262 shape, last (its await defers past the sync tail):
// instance + static computed keys from await, numeric, index read
async function go() {
  class C2 {
    [await 9] = "i";
    static [await 10] = "s2";
  }
  const c: any = new C2();
  console.log(c[9], (C2 as any)[10]);
}
go();
