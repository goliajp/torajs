// §13.2.4 array-literal elision creates a HOLE, not an own
// undefined property: `in` / hasOwnProperty answer false, keys
// skips it, and indexOf/lastIndexOf skip it per §23.1.3's
// HasProperty gating. Pre-fix the elision slot was a real
// undefined element (indexOf(undefined) found it).

const a: any = [0, , 2];
console.log(a.length); // 3
console.log(a[1]); // undefined (reads through the hole)
console.log(1 in a); // false
console.log(a.hasOwnProperty("1")); // false
console.log(Object.keys(a).join(",")); // 0,2
console.log(a.indexOf(undefined)); // -1
console.log(a.lastIndexOf(undefined)); // -1

// includes has NO HasProperty gate (§23.1.3.16) — a hole DOES
// match undefined there
console.log(a.includes(undefined)); // true

// the INLINE-receiver form takes the typed lane; its Any-element
// search now routes through the hole-aware kernels too
console.log([0, , 2].indexOf(undefined)); // -1
console.log([0, , 2].lastIndexOf(undefined)); // -1
console.log([5, 6, 7].indexOf(6)); // 1 (typed lanes keep the inline loop)

// §23.1.3.17 step 9.a — HasProperty walks the CHAIN: an index
// defined on Array.prototype makes the hole visible (a getter-less
// proto entry Gets undefined, which matches)
Object.defineProperty(Array.prototype, "0", { set: function () {}, configurable: true });
console.log(([,] as any).indexOf(undefined)); // 0
delete (Array.prototype as any)["0"];
console.log(([,] as any).indexOf(undefined)); // -1

// a real undefined element still matches
const b: any = [0, undefined, 2];
console.log(b.indexOf(undefined)); // 1
console.log(1 in b); // true

// writes revive the hole
a[1] = 9;
console.log(1 in a, a[1]); // true 9

// trailing elisions count toward length but stay holes
const c: any = [, ,];
console.log(c.length, 0 in c); // 2 false
console.log("done");
