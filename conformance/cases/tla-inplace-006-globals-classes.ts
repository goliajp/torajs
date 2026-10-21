// The module body awaits with top-level bindings of every kind live:
// a `let` a named function reads and the body assigns after an await,
// a class instantiated only after an await, a string and an array
// held across it. All of them stay intact while the jobs ahead of
// each continuation run, and are torn down once the queue is empty.
let counter = 0;
const names: string[] = [];
function bump(by: number): number {
  counter = counter + by;
  return counter;
}

class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  sum(): number {
    return this.x + this.y;
  }
}

const label = "lbl-" + String(counter);
names.push(label);
await null;
counter = 40;
console.log(bump(2), label, names.length);

const p = new Point(await Promise.resolve(3), 4);
names.push("p" + p.sum());
await Promise.resolve(0).then((x) => x + 1);
console.log(p.sum(), names.join("|"), bump(1));

let settle: (s: string) => void = () => {};
const later = new Promise<string>((r) => {
  settle = r;
});
queueMicrotask(() => settle("job"));
console.log(await later, counter);
export {};
