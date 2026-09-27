// A generator body's switch may yield in any clause: the scrutinee
// evaluates once, case expressions evaluate in order until one
// matches, a default written between cases is taken only when none
// does, clauses fall through, `break` leaves the switch and
// `continue` the enclosing loop. It used to be rejected with "yield
// is only valid inside a `function*` generator body".
const trace: string[] = [];
function k(v: number): number {
  trace.push("k" + v);
  return v;
}
function* fall(x: number) {
  switch (x) {
    case 1:
      yield "one";
    case 2:
      yield "two";
      break;
    case 3:
      yield "three";
  }
  yield "end";
}
function* midDefault(x: number) {
  switch (x) {
    case k(1):
      yield "a";
    default:
      yield "d";
    case k(2):
      yield "b";
      break;
    case k(3):
      yield "c";
  }
}
function* inLoop() {
  for (let i = 0; i < 4; i++) {
    switch (i % 3) {
      case 0:
        yield "z" + i;
        continue;
      case 1:
        yield "o" + i;
        break;
      default:
        if (i > 2) break;
        yield "t" + i;
    }
    yield "after" + i;
  }
}
function* labeledOut() {
  outer: for (let i = 0; i < 3; i++) {
    switch (i) {
      case 1:
        yield "hit" + i;
        break outer;
      default:
        yield "miss" + i;
    }
  }
  yield "done";
}
const log: string[] = [];
function* returns(x: number) {
  try {
    switch (x) {
      case 1:
        yield 1;
        return 10;
      default:
        yield 2;
    }
    yield 3;
  } finally {
    log.push("fin" + x);
  }
}
function* letAcross(x: number) {
  switch (x) {
    case 1: {
      let s = "blk" + x;
      yield s;
      yield s + "!";
      break;
    }
    case 2:
      const t = "t" + x;
      yield t;
      yield t + "?";
  }
}
function* scrutOnce() {
  let n = 0;
  switch ((n++, n)) {
    case 1:
      yield "n=" + n;
  }
}
console.log([...fall(1)].join(","), [...fall(2)].join(","), [...fall(3)].join(","), [...fall(9)].join(","));
trace.length = 0;
console.log([...midDefault(2)].join(","), trace.join(","));
trace.length = 0;
console.log([...midDefault(7)].join(","), trace.join(","));
trace.length = 0;
console.log([...midDefault(1)].join(","), trace.join(","));
console.log([...inLoop()].join(","));
console.log([...labeledOut()].join(","));
const r1 = returns(1);
console.log(JSON.stringify([r1.next(), r1.next()]), log.join(","));
console.log([...returns(5)].join(","), log.join(","));
console.log([...letAcross(1)].join(","), [...letAcross(2)].join(","));
console.log([...scrutOnce()].join(","));
function* recv() {
  switch (1) {
    case 1:
      const v = yield "ask";
      yield "got " + v;
  }
}
const rg = recv();
console.log(rg.next().value, rg.next("x").value, rg.next().done);
