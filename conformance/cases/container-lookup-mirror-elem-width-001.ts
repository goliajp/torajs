// The read-only container-key mirror consulted by width_of knew fewer
// method names than the flow-site walk did. A name it was missing fell
// through to the struct-field-fn projection — a key nobody populated,
// and an empty class defaults narrow — so the widened element's f64
// bits came back read as an integer. Only the unbound reads showed it:
// binding the product first goes through the walk, which knew the name.

function take(v: number): number {
  return v;
}

const xs: number[] = [1, 2, 3];
xs[0] = 1.5;

// The three names that had drifted apart.
console.log(take(xs.toSpliced(1, 1)[0]));
console.log(take(xs.valueOf()[0]));
console.log(take(Array.from(xs)[0]));

// Names both sides already agreed on stay right.
console.log(take(xs.slice(0)[0]));
console.log(take(xs.toSorted()[0]));
console.log(take(xs.toReversed()[2]));

// The bound form was never wrong — it is keyed by the walk.
const ys = xs.toSpliced(1, 1);
console.log(ys[0]);
const zs = xs.valueOf();
console.log(zs[0]);

// valueOf answers the receiver itself, so writing through the product
// is visible on both names.
zs[1] = 2.5;
console.log(take(xs[1]), take(zs[1]));

// A product of a product still resolves.
console.log(take(Array.from(xs).toSpliced(0, 0)[0]));
