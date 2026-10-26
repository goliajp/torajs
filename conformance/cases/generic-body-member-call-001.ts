// A method call in a generic function's body whose name only one class
// declares: the specialization calls the receiver's own method, a
// Map's `get` and an interface member, not that class's.
interface J {
  get(k: number): number;
}
class H {
  get(k: number): number {
    return 99;
  }
}
function look<T>(m: Map<number, number>, t: T) {
  return m.get(1);
}
function ask<T>(o: J, t: T) {
  return o.get(2);
}
const m = new Map<number, number>([[1, 5]]);
console.log(look(m, 0), ask({ get: (k: number) => k * 10 }, "x"), new H().get(1));
