// A class declared in a function while the TOP LEVEL binds the same
// name (`const [A, B, E] = mk()`): inside `mk` the name is the class,
// not that outer binding still in its own initializer.
function mk(): any {
  class A { who() { return "A" } }
  class B extends A { who() { return "B>" + super.who() } }
  class E { make() { return new B() } }
  return [A, B, E]
}
const [A, B, E] = mk();
console.log(typeof A, typeof B, typeof E, A.name, B.name, E.name);
console.log(new B().who(), new B() instanceof A, new E().make() instanceof B);
console.log(Object.getPrototypeOf(B) === A, String(new E().make().constructor === B));

function one(): any {
  class K { static make() { return new K() } v = 1 }
  return K
}
const K = one();
console.log(typeof K, K.name, K.make().v, K.make() instanceof K);
