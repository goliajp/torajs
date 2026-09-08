// RFC 20260816-headless-argv-face — a spread call site's runtime
// width. The face's own doc registered this as owed: a head-less body
// with a runtime argv channel was dropped from the tier as soon as
// any of its call sites carried a spread, because the terminal's
// packer works from a statically lowered operand list. The spread
// lane already materializes the whole list into an `Array<Any>`,
// whose slots are exactly the contiguous NaN-boxes that channel
// wants, so argc and the slot base come straight off that array.
const arr = [2, 3];

function ref() {
  console.log(arguments.length, arguments[0], arguments[1], arguments[3]);
}
ref(42, ...[1], ...arr);
ref(...arr);
ref();
ref(...arr, ...arr, 9);

function sum() {
  let t = 0;
  for (let i = 0; i < arguments.length; i++) {
    t = t + Number(arguments[i]);
  }
  return t;
}
console.log(sum(1, ...arr, 4));
console.log(sum(...[10, 20, 30]));

// heap-cell arguments survive the hop through the array's slots
function names() {
  return String(arguments[0]) + "|" + String(arguments[1]);
}
console.log(names(...["x", "y"]));
console.log(names(..."ab"));

// a plain fixed-arity call on the same declaration keeps its own lane
ref(7, 8);
