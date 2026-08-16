// A class whose heritage is a VALUE expression lowers to the ES5
// constructor pattern, so the class binding holds a function
// expression whose `this` needs the knife-2 receiver promotion. Every
// use of that binding therefore has to be a shape the promoted ABI
// survives — and reading the class as a plain value is how a program
// observes that the class evaluated at all.
//
// `typeof K`, a `var` alias, and `export default K` were the three
// value reads with no admitting shape: one of them anywhere in the
// program un-promoted the constructor and left `__this` an unbound
// capture, which the checker reports as
// "closure `__closure_N` references unknown identifier `__this`".

function mk(tag: any): any {
  return class {
    label() {
      return "made-" + tag;
    }
  };
}

class Derived extends mk("a") {}

// typeof — §13.5.3 reads the reference and answers a string.
console.log(typeof Derived);

// a `var` alias — `desugar_var_hoist` splits this into a declaration
// and a statement-position assignment, so the source name's only
// appearance is an assignment right-hand side.
var Alias = Derived;
console.log(new Alias().label());

// the same read from inside a named function: the ES5 class binding is
// minted with a `__`-prefixed name, and desugar-minted names stay
// main-locals unless the data-global gate carves them out.
function readsIt() {
  return typeof Derived;
}
function returnsIt() {
  return Derived;
}
console.log(readsIt(), typeof returnsIt());

// the heritage expression's side effects run once, at class-definition
// time (§15.7.14), and the prototype chain links both sides.
let calls = 0;
class Base {}
class Seq extends (calls++, Base) {}
console.log(calls, typeof Seq);
console.log(Object.getPrototypeOf(Seq) === Base);
console.log(Object.getPrototypeOf(Seq.prototype) === Base.prototype);

// a class EXPRESSION with a value heritage, aliased with `var`.
var Expr1 = class extends mk("b") {};
console.log(new Expr1().label());

export default Derived;
