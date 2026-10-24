// A top-level `for…of` whose body awaits: the iterator's `return()` is
// called once when the loop stops early, by `break` or by a throw out
// of the body after an await (§7.4.9 IteratorClose, §14.7.5.7), in its
// order against the awaits and a `.then` chain, and not when the
// iterator runs out.
const log: string[] = [];
class Counter {
  n: number;
  label: string;
  i = 0;
  constructor(label: string, n: number) {
    this.label = label;
    this.n = n;
  }
  [Symbol.iterator]() {
    return this;
  }
  next(): { value: number; done: boolean } {
    this.i++;
    log.push(this.label + " next " + this.i);
    return { value: this.i, done: this.i > this.n };
  }
  return(): { value: number; done: boolean } {
    log.push(this.label + " return");
    return { value: 0, done: true };
  }
}
Promise.resolve(0)
  .then(() => log.push("then 1"))
  .then(() => log.push("then 2"))
  .then(() => log.push("then 3"));

for (const v of new Counter("a", 5)) {
  const w = await Promise.resolve(v * 10);
  log.push("a body " + w);
  if (v === 2) break;
}

try {
  for (const v of new Counter("b", 5)) {
    await null;
    if (v === 2) throw new Error("stop");
    log.push("b body " + v);
  }
} catch (e) {
  log.push("b caught " + (e as Error).message);
}

for (const v of new Counter("c", 2)) {
  log.push("c body " + (await v));
}

const xs = [1, 2, 3];
let sum = 0;
for (const x of xs) sum += await x;
log.push("sum " + sum);
for (const line of log) console.log(line);
