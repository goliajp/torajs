// labeled jumps in generators: through inner loops and through
// finally (RFC 20260802 upgrade blade part 2 — per-(kind, label)
// F copies + depth-independent labeled gotos)

// labeled continue from an inline inner loop to the outer SM loop
function* p1(): Generator<any> {
  outer: for (let i = 0; i < 3; i++) {
    yield i;
    for (let j = 0; j < 3; j++) {
      if (j == 0) continue outer;
    }
    console.log("unreach");
  }
}
const a = p1();
console.log(a.next().value);
console.log(a.next().value);
console.log(a.next().value);
console.log(a.next().done);

// labeled break through finally
function* p2(): Generator<any> {
  outer: while (true) {
    try {
      yield "a";
      break outer;
    } finally {
      console.log("F");
    }
  }
  yield "after";
}
const b = p2();
console.log(b.next().value);
console.log(b.next().value);
console.log(b.next().done);

// labeled continue through finally
function* p3(): Generator<any> {
  let n = 0;
  outer: while (n < 2) {
    n++;
    try {
      yield n;
      continue outer;
    } finally {
      console.log("F" + n);
    }
  }
}
const c = p3();
console.log(c.next().value);
console.log(c.next().value);
console.log(c.next().done);

// nested finally chain, labeled break runs both copies inside-out
function* p4(): Generator<any> {
  outer: while (true) {
    try {
      try {
        yield 1;
        break outer;
      } finally {
        console.log("inner");
      }
    } finally {
      console.log("outer");
    }
  }
  yield "end";
}
const d = p4();
console.log(d.next().value);
console.log(d.next().value);
console.log(d.next().done);

// labeled break from an inline inner loop, through finally
function* p5(): Generator<any> {
  outer: while (true) {
    try {
      yield 1;
      for (let j = 0; j < 3; j++) {
        if (j == 1) break outer;
      }
    } finally {
      console.log("F5");
    }
  }
  yield "post";
}
const e = p5();
console.log(e.next().value);
console.log(e.next().value);
console.log(e.next().done);

// labeled continue out of a VERBATIM inline try/catch (no finally,
// no yield in try) — the rewritten goto crosses the try freely
function* p6(): Generator<any> {
  outer: for (let i = 0; i < 3; i++) {
    yield i;
    try {
      if (i == 1) continue outer;
      console.log("t" + i);
    } catch (e) {
      console.log("c" + i);
    }
  }
}
const f = p6();
console.log(f.next().value);
console.log(f.next().value);
console.log(f.next().value);
console.log(f.next().done);

// regression: bare break/continue in inner loops stay inner-owned
function* p7(): Generator<any> {
  for (let i = 0; i < 2; i++) {
    yield i;
    let hits = 0;
    for (let j = 0; j < 5; j++) {
      if (j == 1) continue;
      if (j == 3) break;
      hits++;
    }
    console.log("hits" + hits);
  }
}
const g = p7();
console.log(g.next().value);
console.log(g.next().value);
console.log(g.next().done);
