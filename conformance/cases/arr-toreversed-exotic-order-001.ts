// toReversed on an exotic receiver reads high -> low (ES §23.1.3.33
// step 5) — the order is observable through accessor indexes, both
// via .call and via the direct any-receiver method form.
let order: any = [];
let arr: any = [0, "x", 2];
Object.defineProperty(arr, 0, { get: function () { order.push(0); return "a"; } });
Object.defineProperty(arr, 2, { get: function () { order.push(2); return "c"; } });
let r: any = (Array.prototype as any).toReversed.call(arr);
console.log(JSON.stringify(order), JSON.stringify(r));
let order2: any = [];
let arr2: any = [1, 2];
Object.defineProperty(arr2, 0, { get: function () { order2.push(0); return 9; } });
let r2: any = arr2.toReversed();
console.log(JSON.stringify(order2), JSON.stringify(r2));
