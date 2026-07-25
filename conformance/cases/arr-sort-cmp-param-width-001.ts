// The elements sort hands a comparator must arrive at the width that
// comparator's parameters were compiled for.
//
// The wiring that ties the two is a one-way width edge — a receiver's
// elements widen the comparator's parameters, never the reverse — so a
// comparator shared with a fractional array takes f64 while another
// receiver's elements are still integers. The helper fast path refuses
// that case outright (it requires the parameters to equal the element
// type) and falls through to the inline compare, where nothing
// converted them either:
//
//   not yet supported: materialize_operand_fpr called on ValueId
//   holding Gpr
//
// Loud, and the mirror of the disagreement the flatMap callback had.

function ncmp(a: number, b: number): number {
  return a - b;
}

// the fractional receiver is what widens ncmp's parameters
const wide: number[] = [3, 1];
wide[0] = 1.5;
wide.sort(ncmp);
console.log(wide[0], wide[1]);

// the integral receiver sharing that comparator is what used to abort
const narrow: number[] = [5, 4];
narrow.sort(ncmp);
console.log(narrow[0], narrow[1]);

// order reversed — integral receiver seen first
function rcmp(a: number, b: number): number {
  return b - a;
}
const first: number[] = [1, 3, 2];
first.sort(rcmp);
const second: number[] = [2.5, 0.5];
second.sort(rcmp);
console.log(first[0], first[1], first[2], second[0], second[1]);

// toSorted takes the same comparator path
const src: number[] = [7, 6];
const out: number[] = src.toSorted(ncmp);
console.log(out[0], out[1]);

// a comparator used only with integral arrays stays narrow
function icmp(a: number, b: number): number {
  return a - b;
}
const ints: number[] = [9, 8];
ints.sort(icmp);
console.log(ints[0], ints[1]);
