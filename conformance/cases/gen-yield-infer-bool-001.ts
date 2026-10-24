// An unannotated generator that yields booleans only completes with
// undefined, like any other.
var obj = Object.create(null);
var value: any;
function* g() {
  value = yield 'hit' in obj;
  value = yield 'miss' in obj;
}
obj.hit = true;
const it: any = g();
let r = it.next('first');
console.log(r.done, r.value, value);
r = it.next('second');
console.log(r.done, r.value, value);
r = it.next('third');
console.log(r.done, r.value, typeof r.value, value);
function* gb() { yield true }
for (const v of gb()) console.log(v);
const b = gb(); b.next();
console.log(b.next().value === undefined);
