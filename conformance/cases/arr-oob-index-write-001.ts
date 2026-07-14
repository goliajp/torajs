// §10.4.2.1 OrdinarySet — writing past the end of an array grows it.
// That holds for every receiver, including the ones with nowhere to
// write a new pointer back to: the cell is fixed, a grow only swaps
// the data buffer behind it. tr used to refuse those with "out-of-
// bounds index write through a temporary array receiver is not yet
// supported".
const xs: any[] = [1];
const get = (): any[] => xs;

get()[3] = 9;
console.log(xs.length, xs[0], xs[1], xs[2], xs[3]);

// The setup test262 uses for an inherited index property: writing an
// index onto Array.prototype, which is itself an (empty) array.
(Array.prototype as any)[0] = false;
console.log(Array.prototype.length, (Array.prototype as any)[0]);
console.log([true].indexOf(true));
console.log([true, false].lastIndexOf(false));

// An own index still overrides the inherited one.
const ys: any[] = [true];
console.log(ys[0], ys.length);

// A gap fills with undefined rather than staying a hole (tr arrays are
// dense; true sparse storage is a separate substrate, so an index past
// the dense limit still raises RangeError — deliberately not pinned
// here, since bun grows a sparse array instead).
const gap: any[] = [];
gap.push(0);
get()[6] = "x";
console.log(xs.length, xs[5], xs[6]);
