// Two functions may each declare a class of the same name with
// computed members. Each class evaluates only its own computed keys
// and computed static fields (§15.7.14): a generator's `class K` does
// not pick up the keys of another function's `class K`.

function plain(p: string) {
  class K {
    [p]() {
      return 1;
    }
  }
  return (new K() as any)[p]();
}
function* gen(p: string) {
  class K {
    [p]() {
      return 2;
    }
  }
  yield (new K() as any)[p]();
}
console.log(plain("a"));
for (const v of gen("b")) console.log(v);

function* staticOne(k: string) {
  let s = 1;
  class K {
    static [k] = s + 1;
  }
  class Q {
    static [k + "q"] = 3;
  }
  yield (K as any)[k];
  yield (Q as any)[k + "q"];
}
function* staticTwo(k: string) {
  class K {
    static [k] = 5;
  }
  yield (K as any)[k];
}
function* staticAndMethod(k: string) {
  class K {
    static [k] = 6;
    [k + "m"]() {
      return 8;
    }
  }
  yield (K as any)[k];
  yield (new K() as any)[k + "m"]();
}
for (const v of staticOne("z")) console.log(v);
for (const v of staticTwo("a")) console.log(v);
for (const v of staticAndMethod("b")) console.log(v);
