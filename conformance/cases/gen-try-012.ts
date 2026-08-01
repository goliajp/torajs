// GeneratorPrototype.return() routes through enclosing finally copies
// (§27.5.1.7 GeneratorResumeAbrupt with a return completion)
let log1: string[] = [];
function* g(): Generator<any> {
  try {
    yield 1;
    yield 2;
  } finally {
    log1.push("f1");
  }
}
const it = g();
console.log(it.next().value);
let r = it.return("done");
console.log(r.value, r.done);
console.log(log1.join(","));
r = it.next();
console.log(r.value, r.done);

// finally with yield: return() answers F's yields first, then
// completes with the stashed value
let log2: string[] = [];
function* h(): Generator<any> {
  try {
    yield "a";
  } finally {
    log2.push("fh");
    yield "cleanup";
  }
}
const it2 = h();
console.log(it2.next().value);
r = it2.return(7);
console.log(r.value, r.done);
r = it2.next();
console.log(r.value, r.done);
console.log(log2.join(","));

// a return inside F overrides the injected value
function* k(): Generator<any> {
  try {
    yield 1;
  } finally {
    return 100;
  }
}
const it3 = k();
it3.next();
r = it3.return(5);
console.log(r.value, r.done);

// suspendedStart / suspended outside the region: F never runs
let log3: string[] = [];
function* m(): Generator<any> {
  yield 0;
  try {
    yield 1;
  } finally {
    log3.push("fm");
  }
}
const it4 = m();
r = it4.return(3);
console.log(r.value, r.done);
const it5 = m();
console.log(it5.next().value);
r = it5.return(4);
console.log(r.value, r.done);
console.log(log3.length === 0 ? "empty" : log3.join(","));

// nested finallys run inside-out on the injected return
let log4: string[] = [];
function* n(): Generator<any> {
  try {
    try {
      yield "x";
    } finally {
      log4.push("inner");
    }
  } finally {
    log4.push("outer");
  }
}
const it6 = n();
it6.next();
r = it6.return(9);
console.log(r.value, r.done);
console.log(log4.join(","));

// catch never intercepts the return completion; combined
// catch+finally still walks F only
let log5: string[] = [];
function* p(): Generator<any> {
  try {
    yield 1;
  } catch (e) {
    log5.push("never");
  } finally {
    log5.push("fp");
  }
}
const it7 = p();
it7.next();
r = it7.return(2);
console.log(r.value, r.done);
console.log(log5.join(","));

// pure try/catch generator keeps the direct close shape
let log6: string[] = [];
function* q(): Generator<any> {
  try {
    yield 1;
  } catch (e) {
    log6.push("no");
  }
}
const it8 = q();
it8.next();
r = it8.return(11);
console.log(r.value, r.done);

// typed lane: the stashed value unboxes back to the yield type
let log7: string[] = [];
function* t(): Generator<number> {
  try {
    yield 1;
  } finally {
    log7.push("ft");
  }
}
const it9 = t();
it9.next();
const rt = it9.return(42);
console.log(rt.value, rt.done);
console.log(log7.join(","));
