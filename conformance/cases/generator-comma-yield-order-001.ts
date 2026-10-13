// A yield inside a parenthesized comma expression runs after the
// operands before it (§13.16.1), in a loop test, a step, an
// initializer and an expression statement.

function drive(it: any) {
  let r = it.next(0);
  let k = 0;
  while (!r.done) {
    console.log("yielded", r.value);
    k++;
    r = it.next(k * 10);
  }
  console.log("returned", r.value);
}

function* test() {
  let i = 0;
  while ((i++, yield i, i < 3)) {
    console.log("body", i);
  }
  return i;
}
drive(test());

function* step() {
  let j = 0;
  for (; j < 6; (j += 2, yield j)) {
    console.log("step body", j);
  }
}
drive(step());

function* init() {
  let i = 1;
  const x = (i++, yield i, i * 2);
  console.log("x", x);
  const y = (i++, (yield i) + 1, i++, yield i);
  console.log("y", y, i);
}
drive(init());

function* stmt() {
  const log: string[] = [];
  (log.push("a"), yield log.length, log.push("b"), yield log.length);
  console.log(log.join(","));
  let n = 0;
  const z = ((n += 5), n);
  console.log("z", z);
}
drive(stmt());
