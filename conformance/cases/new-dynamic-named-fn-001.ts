// A named function constructing through a top-level `any` binding of
// a class (`const K: any = C; function mk() { return new K() }`): the
// `new` whose constructor is an expression is a reader of that
// binding exactly like a plain read of it, so the binding takes a
// global slot the function body can see. The plain read, the arrow
// spelling and the top-level spelling all worked before; only the
// named-fn `new` said "unknown ident".
class C {
  v = 1;
  bump() { this.v = this.v + 1; return this.v }
}
const K: any = C;
const N: any = 5;
function mk(): any { return new K() }
function mkArg(n: number): any { return new K(n) }
function rd(): any { return N }
const viaArrow = (): any => new K();
console.log(rd(), viaArrow().v, mk().v, mk().bump(), mkArg(3) instanceof C);
console.log(mk() instanceof C, Object.getPrototypeOf(mk()) === C.prototype);
