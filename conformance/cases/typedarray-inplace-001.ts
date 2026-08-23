// §23.2.3 slab A — at / fill / copyWithin / reverse.
//
// Two things here matter beyond "it works": the mutators answer the
// RECEIVER (so a chain keeps mutating one buffer), and `at` takes
// its bounds from the length while an out-of-range index answers
// undefined rather than walking the prototype.
//
// Elements are read back by index rather than with `join`, which
// belongs to the second half of the slab and does not exist yet.

function show(ta: any): string {
  let s = "";
  for (let i = 0; i < ta.length; i++) {
    if (i > 0) s = s + ",";
    s = s + String(ta[i]);
  }
  return s;
}

const a = new Uint8Array([10, 20, 30, 40, 50]);

// at — positive, negative, out of range both ways, absent, fractional
console.log(a.at(0), a.at(4), a.at(-1), a.at(-5));
console.log(a.at(5), a.at(-6), a.at(), a.at(1.9), a.at(-1.9));
console.log(a.at(NaN), a.at(Infinity), a.at(-Infinity));

// fill — whole, ranged, negative range, empty range, out-of-range clamp
console.log(show(new Uint8Array(4).fill(7)));
console.log(show(new Uint8Array([1, 2, 3, 4, 5]).fill(9, 1, 3)));
console.log(show(new Uint8Array([1, 2, 3, 4, 5]).fill(9, -2)));
console.log(show(new Uint8Array([1, 2, 3, 4, 5]).fill(9, 3, 1)));
console.log(show(new Uint8Array([1, 2, 3, 4, 5]).fill(9, -99, 99)));

// fill coerces the way the ELEMENT type says, not the way Number does
console.log(show(new Uint8Array(3).fill(300)));
console.log(show(new Int8Array(3).fill(200)));
console.log(show(new Uint8ClampedArray(4).fill(2.5)));
console.log(show(new Uint8ClampedArray(4).fill(1.5)));
console.log(show(new Float32Array(2).fill(0.1)));
console.log(show(new BigInt64Array(2).fill(-1n)));

// copyWithin — forward, backward, overlapping, negative, no-op
console.log(show(new Uint8Array([1, 2, 3, 4, 5]).copyWithin(0, 3)));
console.log(show(new Uint8Array([1, 2, 3, 4, 5]).copyWithin(2, 0)));
console.log(show(new Uint8Array([1, 2, 3, 4, 5]).copyWithin(0, 1, 3)));
console.log(show(new Uint8Array([1, 2, 3, 4, 5]).copyWithin(-2, 0)));
console.log(show(new Uint8Array([1, 2, 3, 4, 5]).copyWithin(0, 0)));
console.log(show(new Uint8Array([1, 2, 3, 4, 5]).copyWithin(3, 1, 1)));

// reverse — odd, even, single, empty
console.log(show(new Uint8Array([1, 2, 3]).reverse()));
console.log(show(new Uint8Array([1, 2, 3, 4]).reverse()));
console.log(show(new Uint8Array([1]).reverse()));
console.log(show(new Uint8Array(0).reverse()));
console.log(show(new Float64Array([1.5, 2.5, 3.5]).reverse()));

// the mutators answer the receiver, so a chain mutates one buffer
const c = new Uint8Array([1, 2, 3, 4]);
const back = c.fill(5, 2).reverse();
console.log(back === c, show(c), show(back));

// a view mutates its buffer, and its sibling view sees it
const buf = new ArrayBuffer(4);
const v8 = new Uint8Array(buf);
const v16 = new Uint16Array(buf);
v8.fill(255);
console.log(v16[0], v16[1], v8.length, v16.length);
v8.copyWithin(0, 2);
console.log(v8[0], v8[1]);

// an argument's valueOf runs, and runs in the spec's order
const order: string[] = [];
function probe(name: string, v: number): any {
  return {
    valueOf() {
      order.push(name);
      return v;
    },
  };
}
const p = new Uint8Array([1, 2, 3, 4]);
p.fill(probe("value", 8), probe("start", 1), probe("end", 3));
console.log(order.join(">"), show(p));
