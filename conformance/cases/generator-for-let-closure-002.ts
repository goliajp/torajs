// A `for (let …)` binding read by a closure is per iteration
// (§14.7.4.2 CreatePerIterationEnvironment) also where the loop or the
// statement making the closure suspends.

// a yield in the step
function* g1() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++, yield 0) {
    fs.push(() => i);
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...g1()].join(" "));

// a yield in the test
function* g2() {
  const fs: Array<() => number> = [];
  for (let i = 0; (yield 0, i < 3); i++) {
    fs.push(() => i);
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...g2()].join(" "));

// the statement making the closure yields it
function* g3() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    yield fs.push(() => i);
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...g3()].join(" "));

// a yield inside the statement, before the closure is made
function* g4() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    fs.push((yield 0, () => i));
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...g4()].join(" "));

// an `if` whose test makes the closure and whose branch yields
function* g5() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    if (fs.push(() => i) > 0) {
      yield i;
    }
  }
  yield fs.map((f) => f()).join(",");
}
console.log([...g5()].join(" "));

// a value sent in through next() lands in the iteration's binding
function* g6() {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    i = (yield i) as number;
    fs.push(() => i);
  }
  yield fs.map((f) => f()).join(",");
}
const it = g6();
const seen: string[] = [String(it.next().value)];
seen.push(String(it.next(1).value));
seen.push(String(it.next(10).value));
console.log(seen.join(" "));
