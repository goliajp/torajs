// return inside try-with-finally runs F on the way out (§14.13.3
// return completion), including nested chains and returns in catch
let log: string[] = [];
function* g(): Generator<any> {
  try {
    yield 1;
    return "early";
  } finally {
    log.push("f1");
    yield "cleanup";
  }
}
const it = g();
console.log(it.next().value);
console.log(it.next().value);
let r = it.next();
console.log(r.value, r.done);
console.log(log.join(","));

// nested: inner return runs inner F then outer F (outside-in)
function* h(): Generator<any> {
  try {
    try {
      yield "a";
      return 42;
    } finally {
      log.push("inner");
    }
  } finally {
    log.push("outer");
  }
}
const it2 = h();
console.log(it2.next().value);
r = it2.next();
console.log(r.value, r.done);
console.log(log.join(","));

// return in catch (catch+finally combo) still walks F
function* k(): Generator<any> {
  try {
    yield "x";
    throw new Error("e");
  } catch (err) {
    return "from-catch";
  } finally {
    log.push("fk");
  }
}
const it3 = k();
console.log(it3.next().value);
r = it3.next();
console.log(r.value, r.done);
console.log(log.join(","));

// bare return through finally; finally-return override still wins
function* m(): Generator<any> {
  try {
    yield 1;
    return;
  } finally {
    log.push("fm");
  }
}
const it4 = m();
it4.next();
r = it4.next();
console.log(r.value, r.done);
console.log(log.join(","));
