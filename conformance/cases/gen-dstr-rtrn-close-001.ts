// t262 rtrn-close family, minimal: IteratorClose fires when
// gen.return() interrupts a suspended dstr assignment — not at the
// walk (suspend-time returnCount must be 0), with §7.4.6 err
// propagation, the step-9 non-Object TypeError, and the normal-
// completion close.
// basic: return() called once, with an object answer
let cnt1 = 0;
const iter1: any = {
  next() { return { done: false, value: undefined }; },
  return() { cnt1 += 1; return {}; }
};
const ib1: any = {};
ib1[Symbol.iterator] = function() { return iter1; };
function* g1() {
  let x: any;
  [ x = yield ] = ib1;
  console.log("unreachable");
}
const it1 = g1();
it1.next();
console.log("basic pre", cnt1);
const r1: any = it1.return(777);
console.log("basic post", cnt1, r1.value, r1.done);

// err: return() throws — propagates out of gen.return()
const iter2: any = {
  next() { return { done: false, value: undefined }; },
  return() { throw new RangeError("closeboom"); }
};
const ib2: any = {};
ib2[Symbol.iterator] = function() { return iter2; };
function* g2() {
  let x: any;
  [ x = yield ] = ib2;
}
const it2 = g2();
it2.next();
try { it2.return(1); console.log("err no-throw"); }
catch (e: any) { console.log("err", e.constructor.name, e.message); }

// null: return() answers null — TypeError out of gen.return()
let cnt3 = 0;
const iter3: any = {
  next() { return { done: false, value: undefined }; },
  return() { cnt3 += 1; return null; }
};
const ib3: any = {};
ib3[Symbol.iterator] = function() { return iter3; };
function* g3() {
  let x: any;
  [ x = yield ] = ib3;
}
const it3 = g3();
it3.next();
try { it3.return(1); console.log("null no-throw"); }
catch (e: any) { console.log("null", e.constructor.name, cnt3); }

// normal completion still closes exactly once (not-done source)
let cnt4 = 0;
const iter4: any = {
  next() { return { done: false, value: 9 }; },
  return() { cnt4 += 1; return {}; }
};
const ib4: any = {};
ib4[Symbol.iterator] = function() { return iter4; };
function* g4() {
  let x: any;
  [ x = yield ] = ib4;
  return x;
}
const it4 = g4();
it4.next();
console.log("normal", JSON.stringify(it4.next("V")), cnt4);
