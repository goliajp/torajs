// P8.5 — class expressions with inner names (discarded) and the
// `extends` clause. Inner names like `Inner` in `class Inner { ... }`
// are consumed-and-discarded by parse_class_decl_with_abstract's
// `force_synth` mode; the body still resolves through `__ClassExpr_<id>`
// just like the anonymous form. `extends` uses the regular class
// machinery because synth classes appear in ast.stmts via parse_program's
// flush, with the parent class declared in an earlier source stmt.

class Animal {
  kind(): string { return "animal" }
  describe(): string { return "a " + this.kind() }
}

// 1) Inner-name class expression (Inner is silently ignored — the
//    underlying class is __ClassExpr_<id>).
const F1 = class Inner {
  label(): string { return "labeled" }
}
console.log(new F1().label())

// 2) Anonymous class extending a top-level class.
const Dog = class extends Animal {
  kind(): string { return "dog" }
}
const d = new Dog()
console.log(d.kind(), d.describe())

// 3) Named-inner extends. Same shape as (2); just verifies the inner
//    name doesn't interfere with the extends clause parsing.
const Cat = class Felis extends Animal {
  kind(): string { return "cat" }
}
console.log(new Cat().kind(), new Cat().describe())

// 4) Chain alias of an extends class expression — narrow alias-of-alias
//    path. `const Pet = Dog` should map Pet → the underlying synth so
//    `new Pet()` dispatches through the same factory as `new Dog()`.
const Pet = Dog
console.log(new Pet().kind(), new Pet().describe())

// 5) Anonymous class expression with constructor accepting args, then
//    used in a sequence of `new` calls.
const Counter = class {
  n: number
  constructor(start: number) { this.n = start }
  next(): number {
    this.n = this.n + 1
    return this.n
  }
}
const c = new Counter(10)
console.log(c.next(), c.next(), c.next())
