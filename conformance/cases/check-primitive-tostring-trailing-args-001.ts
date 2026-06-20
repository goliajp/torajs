// S293 — Boolean / Symbol / String.{toString,toLocaleString}(...trailing)
// trailing-arg ignore per ES §20.3.3.{2,3} / §20.4.3.{2,3} /
// §22.1.3.{27,28}. Spec sigs are 0-arg; tora's SSA-emit folds to an
// identity-style return (recv_op for String, bool_to_str(recv_op) for
// Boolean, symbol_to_str(recv_op) for Symbol) without inspecting args.
// check.rs S260 typecheck-and-drops trailing operands; ssa_lower's 3
// identity arms now lower-and-drop them so step()-style side-effect
// exprs fire per ES eval-then-discard (S272 idiom).

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

const s: string = "hello";
console.log("s.toString bare:", s.toString());
console.log("s.toString trail:", s.toString(step("s1")));
console.log("calls:", calls);
console.log("s.toString multi:", s.toString(step("s2"), step(42), step(true)));
console.log("calls:", calls);
console.log("s.toLocaleString bare:", s.toLocaleString());
console.log("s.toLocaleString trail:", s.toLocaleString(step("s3")));
console.log("s.toLocaleString multi:", s.toLocaleString(step("s4"), step(99)));
console.log("calls:", calls);

const b: boolean = true;
console.log("b.toString bare:", b.toString());
console.log("b.toString trail:", b.toString(step("b1")));
console.log("b.toString multi:", b.toString(step("b2"), step(7), step("x")));
console.log("b.toLocaleString trail:", b.toLocaleString(step("b3")));
console.log("b.toLocaleString multi:", b.toLocaleString(step("b4"), step("b5")));
console.log("calls:", calls);

const sym: symbol = Symbol("x");
console.log("sym.toString bare:", sym.toString());
console.log("sym.toString trail:", sym.toString(step("sym1")));
console.log("sym.toString multi:", sym.toString(step("sym2"), step("sym3")));
console.log("sym.toLocaleString trail:", sym.toLocaleString(step("sym4")));
console.log("sym.toLocaleString multi:", sym.toLocaleString(step("sym5"), step(11)));
console.log("calls final:", calls);
