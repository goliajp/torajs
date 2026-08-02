// labeled break/continue across an intervening try-finally runs the
// finally on the way out (pending-flag routing in ssa_lower)
function f1(): string {
  const log: string[] = [];
  outer: for (let i = 0; i < 3; i++) {
    try {
      if (i == 1) break outer;
      log.push("t" + i);
    } finally {
      log.push("f" + i);
    }
  }
  return log.join(",");
}
console.log(f1());

function f2(): string {
  const log: string[] = [];
  outer: for (let i = 0; i < 3; i++) {
    try {
      if (i >= 1) continue outer;
      log.push("t" + i);
    } finally {
      log.push("f" + i);
    }
    log.push("post" + i);
  }
  return log.join(",");
}
console.log(f2());

// nested finally chain runs inside-out
function f3(): string {
  const log: string[] = [];
  outer: while (true) {
    try {
      try {
        break outer;
      } finally {
        log.push("inner");
      }
    } finally {
      log.push("outer");
    }
  }
  return log.join(",");
}
console.log(f3());

// labeled BLOCK target
function f4(): string {
  const log: string[] = [];
  blk: {
    try {
      log.push("a");
      break blk;
    } finally {
      log.push("f");
    }
    log.push("unreach");
  }
  log.push("end");
  return log.join(",");
}
console.log(f4());

// finally body itself breaking the label (no intervening finally —
// direct branch, regression face)
function f5(): string {
  const log: string[] = [];
  outer: for (let i = 0; i < 3; i++) {
    try {
      log.push("t" + i);
    } finally {
      if (i == 1) break outer;
    }
  }
  log.push("end");
  return log.join(",");
}
console.log(f5());

// generator: labeled continue out of a verbatim inline try-FINALLY
// (the gen-try-017 p6 original shape, unlocked by this blade)
function* g(): Generator<any> {
  outer: for (let i = 0; i < 3; i++) {
    yield i;
    try {
      if (i == 1) continue outer;
      console.log("t" + i);
    } finally {
      console.log("f" + i);
    }
  }
}
const it = g();
console.log(it.next().value);
console.log(it.next().value);
console.log(it.next().value);
console.log(it.next().done);
