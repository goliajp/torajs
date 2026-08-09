// RFC 20260810 knife 3 — §15.5.5 self-name inside a generator
// expression body (read forms): the wrapper arrow reuses the
// fn-expr's ExprId so the knife-1 self-slot binds the name, and the
// wrapper cell rides into the factory as a leading param the prep
// pass moves onto `this.<name>`.
var g: any = function* rec(n: number): any {
  if (n > 0) {
    yield n;
    yield* rec(n - 1);
  }
};
var it: any = g(3);
console.log(it.next().value, it.next().value, it.next().value, it.next().done);
var t: any = function* me(): any {
  yield typeof me;
};
console.log(t().next().value);
let base = 100;
var h: any = function* add(n: number): any {
  if (n > 0) {
    yield base + n;
    yield* add(n - 1);
  }
};
var it2: any = h(2);
console.log(it2.next().value, it2.next().value, it2.next().done);
