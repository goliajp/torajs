// 刀 D (RFC 20260820) — deferred REST: §13.15.5.5 evaluates the rest
// TARGET's reference (its yield suspends) BEFORE the drain. The t262
// array-rest-iter-rtrn-close mirror: prefix-0 rest over a next()-
// throwing iterator — the walk must not step, GetIterator still runs
// before the suspension, and gen.return() closes exactly once.
let nextCount1 = 0;
let returnCount1 = 0;
const iter1: any = {
  next() { nextCount1 += 1; throw new Error("next boom"); },
  return() { returnCount1 += 1; return {}; }
};
const ib1: any = {};
ib1[Symbol.iterator] = function() { return iter1; };
function* g1() {
  let result: any;
  const obj: any = {};
  result = [...obj[yield]] = ib1;
  console.log("unreachable 1");
}
const it1 = g1();
it1.next();
console.log("rtrn pre", nextCount1, returnCount1);
const r1: any = it1.return(444);
console.log("rtrn post", nextCount1, returnCount1, r1.value, r1.done);

// trlg: one named element before the rest — the walk steps exactly
// once (nextCount must be 1 at the suspension), then closes on
// gen.return().
let nextCount2 = 0;
let returnCount2 = 0;
const iter2: any = {
  next() { nextCount2 += 1; return { done: nextCount2 > 10 }; },
  return() { returnCount2 += 1; return {}; }
};
const ib2: any = {};
ib2[Symbol.iterator] = function() { return iter2; };
function* g2() {
  let x: any;
  let result: any;
  const obj: any = {};
  result = [ x, ...obj[yield] ] = ib2;
  console.log("unreachable 2");
}
const it2 = g2();
it2.next();
console.log("trlg pre", nextCount2, returnCount2);
const r2: any = it2.return(999);
console.log("trlg post", nextCount2, returnCount2, r2.value, r2.done);

// err: rest-form close where return() throws — propagates.
const iter3: any = {
  next() { return { done: false, value: 1 }; },
  return() { throw new RangeError("closeboom"); }
};
const ib3: any = {};
ib3[Symbol.iterator] = function() { return iter3; };
function* g3() {
  const obj: any = {};
  [...obj[yield]] = ib3;
}
const it3 = g3();
it3.next();
try { it3.return(1); console.log("err no-throw"); }
catch (e: any) { console.log("err", e.constructor.name, e.message); }

// null: rest-form close answering null — §7.4.6 step 9 TypeError.
let cnt4 = 0;
const iter4: any = {
  next() { return { done: false, value: 1 }; },
  return() { cnt4 += 1; return null; }
};
const ib4: any = {};
ib4[Symbol.iterator] = function() { return iter4; };
function* g4() {
  const obj: any = {};
  [...obj[yield]] = ib4;
}
const it4 = g4();
it4.next();
try { it4.return(1); console.log("null no-throw"); }
catch (e: any) { console.log("null", e.constructor.name, cnt4); }
