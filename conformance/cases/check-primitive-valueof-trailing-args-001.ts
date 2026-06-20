// S290 — primitive `.valueOf(...trailing)` trailing-arg ignore per
// ES §21.1.3.27 (Number) / §20.4.3.4 (Boolean) / §22.1.3.34
// (String) / §21.2.3.6 (BigInt) / §20.5.3.5 (Symbol) / §23.1.3.34
// (Array). valueOf is 0-arg spec; tora's SSA-emit folds to an
// identity return (recv_op) without inspecting args, so trailing
// operands typecheck-and-drop + lower-and-drop with step() firing
// per ES eval-then-discard semantics.

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

const n: number = 3.14;
console.log("n.valueOf bare:", n.valueOf());
console.log("n.valueOf trail:", n.valueOf(step("n1")));
console.log("calls:", calls);
console.log("n.valueOf multi:", n.valueOf(step("n2"), step(42), step(true)));
console.log("calls:", calls);

const b: boolean = true;
console.log("b.valueOf bare:", b.valueOf());
console.log("b.valueOf trail:", b.valueOf(step("b1")));
console.log("b.valueOf multi:", b.valueOf(step("b2"), step(7)));
console.log("calls:", calls);

const s: string = "hello";
console.log("s.valueOf bare:", s.valueOf());
console.log("s.valueOf trail:", s.valueOf(step("s1")));
console.log("s.valueOf multi:", s.valueOf(step("s2"), step(99), step(false)));
console.log("calls:", calls);

const xs: number[] = [1, 2, 3];
const v1: number[] = xs.valueOf();
console.log("xs.valueOf bare length:", v1.length);
const v2: number[] = xs.valueOf(step("a1"));
console.log("xs.valueOf trail length:", v2.length);
console.log("xs.valueOf trail [0]:", v2[0]);
console.log("calls final:", calls);
