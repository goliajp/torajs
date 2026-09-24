// A map whose product cannot hole still reads each index through [[Get]]:
// an accessor index runs its getter, including one defined mid-walk.
var a2 = [0, , 2];
Object.defineProperty(a2, "0", { get: function() { Object.defineProperty(a2, "1", { get: function() { return 1; }, configurable: true }); return 0; }, configurable: true });
const seen: any[] = [];
a2.forEach((v: any, i: any) => seen.push(i + ":" + v));
console.log(seen.join());
var a3: any = [0, , 2];
Object.defineProperty(a3, "0", { get: function() { Object.defineProperty(a3, "1", { get: function() { return 1; }, configurable: true }); return 0; }, configurable: true });
console.log(a3.map((v: any, i: any) => i + ":" + v).join());
var a4 = [0, , 2];
Object.defineProperty(a4, "1", { get: function() { return 1; }, configurable: true });
console.log(a4.map((v: any, i: any) => i + ":" + v).join(), 1 in a4, a4[1]);
