// `Array.prototype.splice(start, deleteCount?, ...)` per ES §23.1.3.31:
// both `start` and `deleteCount` are spec-optional. Pre-fix tr required
// strictly 2 args and type-rejected the 0-arg / 1-arg forms ("expected
// 2 argument(s), got 1"), missing the spec defaults — `splice(start)`
// deletes from `start` to the end, `splice()` is a no-op. The runtime
// helper already clamps deleteCount to `len - actualStart`, so the
// 1-arg form passes `i64::MAX` as a "delete the rest" sentinel.

const a = [1, 2, 3, 4, 5];
const r1 = a.splice(2);
console.log("splice(2):", JSON.stringify(a), JSON.stringify(r1));

const b = [10, 20, 30, 40, 50];
const r2 = b.splice(-2);
console.log("splice(-2):", JSON.stringify(b), JSON.stringify(r2));

const c = [1, 2, 3];
const r3 = c.splice(0);
console.log("splice(0):", JSON.stringify(c), JSON.stringify(r3));

const d = [1, 2, 3];
const r4 = d.splice();
console.log("splice():", JSON.stringify(d), JSON.stringify(r4));

const e = [1, 2, 3];
const r5 = e.splice(10);
console.log("splice(10):", JSON.stringify(e), JSON.stringify(r5));

// 2-arg form remains unchanged.
const f = [1, 2, 3, 4, 5];
const r6 = f.splice(1, 2);
console.log("splice(1,2):", JSON.stringify(f), JSON.stringify(r6));

const g: string[] = ["a", "b", "c"];
const r7 = g.splice(1);
console.log("splice(str,1):", JSON.stringify(g), JSON.stringify(r7));
