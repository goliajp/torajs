// §23.1.3.22 / §23.1.3.27: pop / shift read the element through [[Get]]
// before the length Set — a getter reached through a hole runs first.
var array: any = new Array(1);
var calls = 0;
Object.defineProperty(Array.prototype, "0", { get() { Object.freeze(array); calls++; }, configurable: true });
try { array.shift(); console.log("no throw"); } catch (e: any) { console.log(e instanceof TypeError); }
console.log(array.length, calls);
var b: any = new Array(1);
var c2 = 0;
Object.defineProperty(Array.prototype, "0", { get() { Object.defineProperty(b, "length", { writable: false }); c2++; }, configurable: true });
try { b.pop(); console.log("no throw"); } catch (e: any) { console.log(e instanceof TypeError); }
console.log(b.length, c2);
delete (Array.prototype as any)[0];
const d: any = [1, , 3];
console.log(d.shift(), d.length, 0 in d, 1 in d);
const e2 = new Array(3); e2[0] = "a";
console.log(e2.pop(), e2.shift(), e2.length);
