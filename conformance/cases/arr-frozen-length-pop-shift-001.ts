// RFC 20260713-array-proto-residual blade 4 — §23.1.3.{20,29}
// pop/shift on a frozen array or non-writable length throws
// TypeError (including the EMPTY receiver — step 3.b still writes
// length 0); unlocked receivers keep the fast path.
var a: number[] = [];
Object.freeze(a);
try { a.pop(); console.log("pop no throw"); } catch (e) { console.log("pop threw:", e instanceof TypeError); }
var b: number[] = [];
Object.freeze(b);
try { b.shift(); console.log("shift no throw"); } catch (e) { console.log("shift threw:", e instanceof TypeError); }
var c = [1, 2];
Object.freeze(c);
try { c.pop(); console.log("pop2 no throw"); } catch (e) { console.log("pop2 threw:", e instanceof TypeError, "len:", c.length); }
var d0: number[] = [];
var d: any = d0;
Object.freeze(d);
try { d.pop(); console.log("anypop no throw"); } catch (e) { console.log("anypop threw:", e instanceof TypeError); }
var e2 = [5];
Object.defineProperty(e2, "length", { writable: false });
try { e2.shift(); console.log("rolen no throw"); } catch (er) { console.log("rolen threw:", er instanceof TypeError, "len:", e2.length); }
// unlocked arrays keep working
var g = [1, 2, 3];
console.log("pop:", g.pop(), "shift:", g.shift(), "len:", g.length);
