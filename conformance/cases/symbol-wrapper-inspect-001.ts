// console.log(Object(sym)) — bun expands the SymbolWrapper as a
// fixed four-field multi-line block (description + the reified
// prototype surface); nested contexts pad at container indent + 2.
const w1 = Object(Symbol("hi"));
console.log(w1);
const w2 = Object(Symbol());
console.log(w2);
console.log([w1]);
console.log({ x: w1 });
