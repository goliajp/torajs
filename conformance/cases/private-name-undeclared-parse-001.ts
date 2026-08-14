// §13.1 AllPrivateNamesValid — a `#x` reference inside a class body
// that no enclosing class declares is an early SyntaxError, at PARSE.
//
// It used to be left to the checker, which recognizes the mangled name
// only on a receiver typed as a class. That made a parse-phase early
// error depend on the STATIC TYPE of `this`, and the receiver-promoting
// knives walked straight into it: once a function expression's `this`
// is the call-site receiver it types `any`, the class arm stops
// matching, and `class C { f = function () { this.#x } }` went from
// correctly refused to silently accepted (test262
// class/elements/syntax/early-errors/invalid-names/*-fn-member-
// expression-this, four cases).
//
// This case pins the half that must keep WORKING: a reference that
// does resolve, including through an enclosing class and through the
// promoted-receiver shapes the same rotation opened up.

class Holder {
  #v = 41;
  read(): number { return this.#v }
  bump(): number { this.#v += 1; return this.#v }
}
const h = new Holder();
console.log(1, h.read(), h.bump());

// An INNER class body sees the outer class's private name (§15.7): the
// stack of enclosing private scopes is what resolves it, not the
// innermost one alone.
// (The reference is what is under test — reaching it is enough. CALLING
// the inner method through an `any` parameter lands on a member-call
// shape the lowering does not serve yet, which is a different row.)
class Outer {
  #y = 7;
  peek(): any {
    class Inner {
      take(o: any): number { return o.#y }
    }
    return typeof new Inner();
  }
}
console.log(2, new Outer().peek());

// An inner redeclaration shadows — same caveat about calling through
// the nested class as above; what is pinned is that both references
// resolve and the outer read still answers the outer field.
class Shadow {
  #n = 1;
  probe(): any {
    class Deep {
      #n = 2;
      get(): number { return this.#n }
    }
    return [this.#n, typeof new Deep()];
  }
}
console.log(3, new Shadow().probe());

// `#x in o` — the brand check, whose own reference resolves the same
// way.
class Brand {
  #b = 0;
  static has(o: any): boolean { return #b in o }
}
console.log(4, Brand.has(new Brand()), Brand.has({}));

// A static private name, read from a static body.
class Stat {
  static #s = 5;
  static read(): number { return Stat.#s }
}
console.log(5, Stat.read());
