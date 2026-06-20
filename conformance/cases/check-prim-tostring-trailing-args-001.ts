// ES §20.3.3.{2,3} / §20.4.3.3 / §22.1.3.{27,28} — Boolean /
// Symbol / String prototype .toString() / .toLocaleString()
// trailing-arg ignore. Spec sigs are 0-arg; tora's SSA-emit
// already silent-drops user args. S262 widens checktime sigs.

const b: boolean = true;
const bf: boolean = false;
const s: symbol = Symbol("x");
const sn: symbol = Symbol("");
const str: string = "hello";
const empty: string = "";

// Boolean — trailing dropped.
console.log(b.toString("extra"));
console.log(bf.toString(99, "ignored"));
console.log(b.toLocaleString("extra"));
console.log(bf.toLocaleString(1, 2, 3));

// Symbol — trailing dropped.
console.log(s.toString("extra"));
console.log(sn.toString(99));
console.log(s.toLocaleString("extra", "ignored"));
console.log(sn.toLocaleString());

// String — identity passthrough; trailing dropped.
console.log(str.toString("extra"));
console.log(empty.toString(99, "ignored"));
console.log(str.toLocaleString("extra"));
console.log(empty.toLocaleString(1, 2, 3));
