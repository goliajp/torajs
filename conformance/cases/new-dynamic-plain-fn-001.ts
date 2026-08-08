var mk = function (n: any) { return; };
const o = new (mk as any)(1);
console.log(typeof o);
console.log(Object.getPrototypeOf(o) === (mk as any).prototype);
var mk3 = function (): any { return { y: 7 }; };
const q = new (mk3 as any)();
console.log((q as any).y);
var mk4 = function (): any { return 42; };
const r = new (mk4 as any)();
console.log(typeof r);
const arrow: any = () => 1;
try { new arrow(); console.log(1); } catch (e) { console.log(2); }
