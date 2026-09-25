// a program that hands a generator's return() a value keeps its
// unannotated generators on the any lane: return(v) answers v out of
// the same slot the yields fill, whatever v is.
function* nums() {
  yield 1;
  yield 2;
}
const a = nums();
a.next();
console.log(JSON.stringify(a.return(5)), a.next().done);
const b = nums();
console.log(JSON.stringify(b.return("x" as any)));
const c = nums();
console.log(JSON.stringify(c.return(undefined)), [...nums()]);
