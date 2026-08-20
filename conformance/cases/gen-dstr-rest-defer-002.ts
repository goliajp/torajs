// 刀 D (RFC 20260820) — deferred REST, resume paths: the drain runs
// AFTER the yield resumes, in target-first order.
// resume over a user iterator: prefix binds 1 step, the rest drains
// the remainder to done, and a drained iterator is NOT closed.
let nextCount1 = 0;
let returnCount1 = 0;
let n1 = 0;
const iter1: any = {
  next() {
    nextCount1 += 1;
    n1 += 1;
    const r: any = n1 <= 4 ? { done: false, value: n1 * 10 } : { done: true, value: undefined };
    return r;
  },
  return() { returnCount1 += 1; return {}; }
};
const ib1: any = {};
ib1[Symbol.iterator] = function() { return iter1; };
function* g1() {
  let x: any;
  const obj: any = {};
  [ x, ...obj[yield] ] = ib1;
  console.log("resume", x, JSON.stringify(obj.k), nextCount1, returnCount1);
}
const it1 = g1();
it1.next();
console.log("suspend", nextCount1, returnCount1);
it1.next("k");

// any-Array source: the indexed lane parks a resume index; the rest
// picks up past the prefix.
function* g2() {
  const src: any = [1, 2, 3, 4];
  let a: any;
  const obj: any = {};
  [ a, ...obj[yield] ] = src;
  console.log("arr", a, JSON.stringify(obj.k));
}
const it2 = g2();
it2.next();
it2.next("k");

// prefix-0 over an any-Array: never stepped at the suspension, the
// drain walks the whole source.
function* g3() {
  const src: any = [7, 8, 9];
  const obj: any = {};
  [...obj[yield]] = src;
  console.log("arr0", JSON.stringify(obj.k));
}
const it3 = g3();
it3.next();
it3.next("k");

// thrw-close-skip: the drain's next() throws mid-drain — [[done]] is
// already true, so the close must NOT fire on the way out.
let returnCount4 = 0;
let n4 = 0;
const iter4: any = {
  next() {
    n4 += 1;
    if (n4 > 2) { throw new Error("mid-drain boom"); }
    return { done: false, value: n4 };
  },
  return() { returnCount4 += 1; return {}; }
};
const ib4: any = {};
ib4[Symbol.iterator] = function() { return iter4; };
function* g4() {
  let x: any;
  const obj: any = {};
  [ x, ...obj[yield] ] = ib4;
  console.log("unreachable 4");
}
const it4 = g4();
it4.next();
try { it4.next("k"); console.log("thrw no-throw"); }
catch (e: any) { console.log("thrw", e.message, returnCount4); }

// lref-throw keeps the close: the rest target's KEY expression
// throws after the resume (the yield sits in object position, so
// the source order matches the spec's reference evaluation) — the
// iterator is still parked (not done), so the pattern's finally
// owes it the close.
let returnCount5 = 0;
const iter5: any = {
  next() { return { done: false, value: 1 }; },
  return() { returnCount5 += 1; return {}; }
};
const ib5: any = {};
ib5[Symbol.iterator] = function() { return iter5; };
function boomKey(): any { throw new Error("kref boom"); }
function* g5() {
  let x: any;
  [ x, ...(yield)[boomKey()] ] = ib5;
  console.log("unreachable 5");
}
const it5 = g5();
it5.next();
try { it5.next({}); console.log("lref no-throw"); }
catch (e: any) { console.log("lref", e.message, returnCount5); }
