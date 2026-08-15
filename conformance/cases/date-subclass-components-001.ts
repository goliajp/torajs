// Date-subclass components ctor — §21.4.2.1 step 6 via the
// synthesized rest default ctor (rotation 413 blade: the 2+-argument
// arm hands the packed rest array to the components kernel; day
// defaults to 1 and the time components to 0 when the list stops
// short, and MakeFullYear maps two-digit years).
class D extends Date {}
const d = new D(2016, 6);
console.log(d.getFullYear());
console.log(d.getMonth());
console.log(d.getDate());
console.log(d.getHours());
const d2 = new D(2016, 6, 15, 10, 30, 20, 500);
console.log(d2.getDate());
console.log(d2.getHours());
console.log(d2.getMinutes());
console.log(d2.getSeconds());
console.log(d2.getMilliseconds());
const d3 = new D(99, 0);
console.log(d3.getFullYear());
// a present undefined component is NOT a missing one — ToNumber(NaN)
// clips to Invalid Date, exactly the plain ctor's account
const d4 = new D(2016, undefined as any);
console.log(d4.getTime());
console.log(d instanceof D);
console.log(d instanceof Date);
