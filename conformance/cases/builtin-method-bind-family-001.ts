// Rotation 207 — a bound reified builtin method must keep the
// prototype family it was minted for. The bound cell used to store
// the bare method id, which dropped the family and therefore both
// the family-generic gate (§21.1.3 thisNumberValue / §20.3.3
// thisBooleanValue brand checks) and the family-aware `length`
// reflection (§21.1.6.6 Number.prototype.toString has length 1,
// every other prototype's toString has 0).

// Brand checks survive .bind — a wrong-brand receiver throws exactly
// as it does through .call.
try {
  console.log("A no-throw", Boolean.prototype.toString.bind({})());
} catch (e) {
  console.log("A", e instanceof TypeError);
}
try {
  console.log("B no-throw", Number.prototype.toString.bind({})());
} catch (e) {
  console.log("B", e instanceof TypeError);
}
// .call through a binding reaches the same gate (the inline
// `Boolean.prototype.toString.call({})` callee shape is a separate
// station, tracked on its own).
const borrowed = Boolean.prototype.toString;
try {
  console.log("C no-throw", borrowed.call({}));
} catch (e) {
  console.log("C", e instanceof TypeError);
}

// Right-brand receivers still dispatch.
console.log("D", Number.prototype.toString.bind(255)(16));
console.log("E", Number.prototype.toFixed.bind(3.14159)(2));
console.log("F", Boolean.prototype.toString.bind(false)());
console.log("G", String.prototype.slice.bind("abcdef")(1, 3));

// length is family-aware: Number's toString is 1, the others are 0.
console.log("H", Number.prototype.toString.length);
console.log("I", String.prototype.toString.length);
console.log("J", Boolean.prototype.toString.length);
console.log("K", Number.prototype.toFixed.length);
console.log("L", String.prototype.slice.length);

// Bound length subtracts the partially applied arguments.
console.log("M", Number.prototype.toString.bind(255).length);
console.log("N", String.prototype.slice.bind("abcdef").length);
console.log("O", String.prototype.slice.bind("abcdef", 1).length);

// name reflection is unchanged by the packing.
console.log("P", Number.prototype.toString.name);
console.log("Q", Number.prototype.toString.bind(255).name);
console.log("R", typeof Number.prototype.toString.bind(255));
