// Adapted from test262 patterns: a generic class implementation of
// a stack. Exercises class<T> + push/peek over an internal array.
// Demonstrates fixed-point monomorphization on a class with one
// method that mutates the array field. Subset constraint: tr
// doesn't yet parse explicit `<T>` after `new`, so the constructor
// takes a "seed" arg whose type drives inference.
class Stack<T> {
  data: T[];
  constructor(seed: T) {
    let init: T[] = [];
    this.data = init;
    this.data.push(seed);
  }
  add(v: T): void {  // Renamed from `push` to dodge tr's
    this.data.push(v);  // method-name desugar collision with Array.push.
  }
  size(): number {
    return this.data.length;
  }
}

function check(): number {
  let s = new Stack(0);  // T inferred as number from the seed.
  if (s.size() !== 1) { throw "#1: seed"; }

  s.add(10);
  s.add(20);
  s.add(30);
  if (s.size() !== 4) { throw "#2"; }
  return 0;
}
console.log(check());
