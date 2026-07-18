// rotation 140 — indexOf / lastIndexOf abrupt-completion ordering.
// test262 15.4.4.14-5-30 / 15.4.4.15-5-30 (stepFiveOccurs) +
// 15.4.4.14-9-b-i-30/31, 15.4.4.15-8-b-i-30/31 (accessed): a
// throwing getter (index accessor or length valueOf) recorded the
// pending throw but the scan kept running — later getters and the
// fromIndex ToInteger valueOf fired their observable side effects
// before the throw propagated. Fix: __torajs_throw_check() aborts
// the scan right after every observable step (arraylike_len's
// ToNumber, per-index Gets in both the dynobj generic-receiver
// loops and the real-Array any-tier kernels, and the fromIndex
// to_index in the Tag::Arr dispatch arms).

// A: length-valueOf throw must precede the fromIndex valueOf.
let order: string[] = [];
const obj: any = {};
Object.defineProperty(obj, "length", {
  get: function() { order.push("len-get"); return { valueOf: function() { order.push("len-valueOf"); throw new TypeError("boom"); } }; },
  configurable: true
});
const fromIndex = { valueOf: function() { order.push("from-valueOf"); return 0; } };
try {
  (Array.prototype.indexOf as any).call(obj, undefined, fromIndex);
} catch (e: any) { order.push("caught:" + (e instanceof TypeError)); }
console.log("A:", order.join(","));

// B: real-Array scan stops at the index-0 throwing getter; the
// decoy getter at index 1 must never run.
let order2: string[] = [];
const arr: any = [];
Object.defineProperty(arr, "0", { get: function() { order2.push("g0"); throw new TypeError("boom"); }, configurable: true });
Object.defineProperty(arr, "1", { get: function() { order2.push("g1"); return true; }, configurable: true });
try { arr.indexOf(true); } catch (e: any) { order2.push("caught:" + (e instanceof TypeError)); }
console.log("B:", order2.join(","));

// C: same shape over a generic object receiver.
let order3: string[] = [];
const obj3: any = { length: 2 };
Object.defineProperty(obj3, "0", { get: function() { order3.push("g0"); throw new TypeError("boom"); }, configurable: true });
Object.defineProperty(obj3, "1", { get: function() { order3.push("g1"); return true; }, configurable: true });
try { (Array.prototype.indexOf as any).call(obj3, true); } catch (e: any) { order3.push("caught:" + (e instanceof TypeError)); }
console.log("C:", order3.join(","));

// D: lastIndexOf scans backwards — throws at 2, never reads 1.
let order4: string[] = [];
const arr4: any = [];
Object.defineProperty(arr4, "2", { get: function() { order4.push("g2"); throw new TypeError("boom"); }, configurable: true });
Object.defineProperty(arr4, "1", { get: function() { order4.push("g1"); return true; }, configurable: true });
try { arr4.lastIndexOf(true); } catch (e: any) { order4.push("caught:" + (e instanceof TypeError)); }
console.log("D:", order4.join(","));

// E: non-throwing controls — the checks must not change the
// happy-path answers.
const plain: any = [10, 20, 30, 20];
console.log("E:", plain.indexOf(20), plain.lastIndexOf(20), plain.includes(30), plain.indexOf(99));
