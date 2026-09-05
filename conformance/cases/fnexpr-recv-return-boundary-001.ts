// Returning a `this`-using binding out of a function. The shape was
// admitted only when TWO annotations were written: the return boundary
// had to be inferred or spelled `any`, AND the binding itself had to
// be spelled `any`. The second half was a hard reject for every
// program that did not write it — `function give() { return kind }`
// with a plain `let kind = function () {...}` did not build.
//
// Only the boundary carries the proof. The single call lane that does
// not read the promoted callee's receiver flag is the bare
// CallIndirect for a `Type::FnSig` callee, and that type only exists
// when a concrete signature is SPELLED — which is exactly what the
// return annotation decides. The binding's own annotation was
// supposed to be what put the returned cell in the any lane; it is
// not, because 398-06 put the same runtime flag test on the typed
// indirect lanes, which is why the array-element and `any`-parameter
// shapes never asked for it either.
let kind = function () {
  return typeof (this as any);
};

function give() {
  return kind;
}

// Detached call: no base, so no receiver (§10.2.1.2).
console.log(give()());

// Through a variable, and through one more boundary.
const held = give();
console.log(held());
function giveAgain() {
  return give();
}
console.log(giveAgain()());

// Landed in a container, the container is the receiver.
const inArray = [give()];
console.log(inArray[0]());
const inField = { f: give() };
console.log(inField.f());

// Passed on as an argument.
function take(f: any) {
  return f();
}
console.log(take(give()));

// Never called at all.
console.log(typeof give());

// ARGUMENTS land after the receiver. A zero-parameter callee cannot
// witness a shifted argv — every probe above would read the same
// either way — so this is the one that would catch it.
let pair = function (x: any, y: any) {
  return typeof (this as any) + "/" + x + "/" + y;
};
function givePair() {
  return pair;
}
console.log(givePair()(7, 8));
console.log(givePair()(7));
// Landed in a container and called back with arguments. The base is
// written as a named binding on purpose: bun 1.4.1 answers
// `undefined` for an ARRAY-LITERAL base (`[f][0]()`), where §13.3.6.2
// makes the literal the receiver — tr and node 26.8.1 both answer
// with the array. That disagreement is bun's, and asserting it here
// would pin this fixture to it.
const heldPair = givePair();
const pairs = [heldPair];
console.log(pairs[0](7, 8));

// Native widths, not just boxes: an f64 parameter through the same
// boundary.
let half = function (x: number) {
  return x / 4;
};
function giveHalf() {
  return half;
}
console.log(giveHalf()(9));

// The return can sit anywhere a return can sit, and there can be
// more than one of them.
let other = function () {
  return "other";
};
function pick(b: boolean) {
  if (b) {
    return kind;
  }
  return other;
}
console.log(pick(true)(), pick(false)());
function fromBlock() {
  {
    return kind;
  }
}
console.log(fromBlock()());

// The explicitly-annotated half still works — it was never the
// problem, only the requirement that it be written.
let named: any = function () {
  return typeof (this as any);
};
function giveNamed(): any {
  return named;
}
console.log(giveNamed()());
