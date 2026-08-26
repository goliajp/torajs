// r503 — a class program whose cells stay private: the prologue
// cells' exit release (`__torajs_class_cell_release`) rides the
// register call's guard and is NOPed with it, the undefined
// `__new_target` box every `new` site mints for its ctor is released
// by the mid-end, and the prototype literal's stores are the insert-
// only fresh kernel. Everything below still runs for real: a ctor
// with an early return, a subclass override, a loop of mints, the
// struct print.
class Counter {
  n = 0;
  constructor(start: number) {
    if (start < 0) {
      this.n = 0;
      return;
    }
    this.n = start;
  }
  bump(): number {
    this.n += 1;
    return this.n;
  }
}
class Twice extends Counter {
  bump(): number {
    this.n += 2;
    return this.n;
  }
}
const c = new Counter(3);
console.log(c.bump(), c.bump());
const t = new Twice(10);
console.log(t.bump(), t.bump());
const z = new Counter(-1);
console.log(z.bump());
let total = 0;
for (let i = 0; i < 500; i++) {
  const k = new Counter(i);
  total += k.bump();
}
console.log(total);
console.log(c);
console.log(t);
