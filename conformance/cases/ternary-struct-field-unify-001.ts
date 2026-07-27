// Ternary branch unification at struct FIELD depth (S2.27 field-depth
// variant): two structs with the same field names and field-wise
// joinable types join to Any — both branches box expr-aware and reads
// go through the any-member runtime-layout path, so a Number field on
// one side and an Undefined/Null field on the other answer correctly
// whichever branch was taken. Covers the iterator idiom (fn-return
// form), direct-let joins for every field combo (number/undefined,
// string/undefined, number/null, nested struct depth), and both
// branch polarities.
function makeIter() {
  let i = 0;
  return {
    next() {
      i++;
      return i <= 3 ? {value: i, done: false} : {value: undefined, done: true};
    }
  };
}
const it = makeIter();
let r = it.next();
while (!r.done) {
  console.log("got", r.value);
  r = it.next();
}
console.log("end", r.value, r.done);

const d1 = false ? {v: 1} : {v: undefined};
console.log("d1", d1.v);
const d2 = true ? {v: 1} : {v: undefined};
console.log("d2", d2.v);
const d3 = false ? {t: "x"} : {t: undefined};
console.log("d3", d3.t);
const d4 = true ? {t: "x"} : {t: undefined};
console.log("d4", d4.t);
const d5 = false ? {v: 1, done: false} : {v: undefined, done: true};
console.log("d5", d5.v, d5.done);
const d6 = true ? {a: {b: 1}} : {a: {b: undefined}};
console.log("d6", d6.a.b);
const d7 = false ? {a: {b: 1}} : {a: {b: undefined}};
console.log("d7", d7.a.b);
const m = false ? {v: 1} : {v: null};
console.log("m", m.v);
const m2 = true ? {v: 1} : {v: null};
console.log("m2", m2.v);
