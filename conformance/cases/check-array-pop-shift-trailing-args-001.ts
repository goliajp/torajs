// S288 — Array<T>.{pop,shift}(...trailing) trailing-arg ignore per
// ES §23.1.3.{20,24}. Spec sigs are 0-arg; tora's pop / shift
// runtime helpers do in-place len decrement (pop) and head-offset
// bump (shift) reading only `recv`, so trailing operands typecheck-
// and-drop + lower-and-drop with step() firing per ES eval-then-
// discard semantics.

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

// pop — single trailing
const xs: number[] = [1, 2, 3, 4, 5];
console.log("xs.pop():", xs.pop(step("extra")));
console.log("xs length:", xs.length);
console.log("calls after pop:", calls);

// pop — multi trailing
console.log("xs.pop multi:", xs.pop(step("e1"), step(42), step(true)));
console.log("xs length:", xs.length);
console.log("calls after pop multi:", calls);

// pop — no trailing (baseline regression check)
console.log("xs.pop bare:", xs.pop());
console.log("xs length:", xs.length);
console.log("calls after pop bare:", calls);

// shift — single trailing
const ys: number[] = [10, 20, 30, 40, 50];
console.log("ys.shift:", ys.shift(step("s-extra")));
console.log("ys length:", ys.length);
console.log("calls after shift:", calls);

// shift — multi trailing
console.log("ys.shift multi:", ys.shift(step("s1"), step(7), step(false)));
console.log("ys length:", ys.length);
console.log("calls after shift multi:", calls);

// shift — no trailing (baseline regression check)
console.log("ys.shift bare:", ys.shift());
console.log("ys length:", ys.length);
console.log("calls final:", calls);
