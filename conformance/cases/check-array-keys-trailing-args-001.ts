// S292 — Array<T>.keys(...trailing) trailing-arg ignore per ES
// §23.1.3.16. Spec sig is 0-arg returning an ArrayIterator (tora:
// Type::ArrIter via arr_iter_create_keys, fed only `recv_op`).
// Trailing operands typecheck-and-drop + lower-and-drop so step()
// fires per ES eval-then-discard.

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

const xs: number[] = [10, 20, 30];

// Baseline 0-arg
const k1 = xs.keys();
console.log("k1.next:", k1.next().value);
console.log("k1.next:", k1.next().value);

// Single trailing
const k2 = xs.keys(step("extra"));
console.log("k2.next:", k2.next().value);
console.log("calls after k2:", calls);

// Multi trailing
const k3 = xs.keys(step("e1"), step(42), step(true));
console.log("k3.next:", k3.next().value);
console.log("calls after k3:", calls);

// for-of consumption with trailing
let sum = 0;
let cnt = 0;
for (const k of xs.keys(step("loop"))) {
    sum = sum + k;
    cnt = cnt + 1;
}
console.log("sum:", sum);
console.log("cnt:", cnt);
console.log("calls final:", calls);
