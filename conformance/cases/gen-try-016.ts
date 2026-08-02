// return / jumps inside an INLINE (yield-free) inner loop route
// through finally correctly — the dispatch loop is labeled (__sm)
// so the routed goto binds past the inner loop (RFC 20260802
// upgrade blade)
function* g1(): Generator<any> {
  try {
    yield 1;
    for (let i = 0; i < 3; i++) {
      if (i == 1) return "r" + i;
    }
  } finally {
    console.log("F1");
  }
}
const a = g1();
let s = a.next();
console.log(s.value, s.done);
s = a.next();
console.log(s.value, s.done);
s = a.next();
console.log(s.value, s.done);

// while-form inner loop
function* g2(): Generator<any> {
  try {
    yield "a";
    let k = 0;
    while (k < 5) {
      k++;
      if (k == 2) return k * 10;
    }
  } finally {
    console.log("F2");
  }
}
const b = g2();
console.log(b.next().value);
s = b.next();
console.log(s.value, s.done);

// nested finally chain: inner-loop return runs inner F then outer F
function* g3(): Generator<any> {
  try {
    try {
      yield 1;
      for (let i = 0; i < 2; i++) {
        if (i == 0) return "deep";
      }
    } finally {
      console.log("inner");
    }
  } finally {
    console.log("outer");
  }
}
const c = g3();
console.log(c.next().value);
s = c.next();
console.log(s.value, s.done);

// return inside an inner loop in a CATCH body under finally (D2)
function* g4(): Generator<any> {
  try {
    try {
      yield 1;
      throw new Error("boom");
    } catch (e) {
      for (let i = 0; i < 3; i++) {
        if (i == 2) return "c" + i;
      }
    }
  } finally {
    console.log("F4");
  }
}
const d = g4();
console.log(d.next().value);
s = d.next();
console.log(s.value, s.done);

// regression: return in a yield-BEARING (state-machined) loop
function* g6(): Generator<any> {
  try {
    for (let i = 0; i < 3; i++) {
      yield i;
      if (i == 1) return "loop" + i;
    }
  } finally {
    console.log("F6");
  }
}
const f = g6();
console.log(f.next().value);
console.log(f.next().value);
s = f.next();
console.log(s.value, s.done);
