// S287 — Array<T>.{reverse,toReversed,join,toString,toLocaleString}(
// ...trailing) trailing-arg ignore per ES §23.1.3.{27,33,15,32,31}.
// Spec sigs are 0-arg (reverse / toReversed / toString / toLocaleString)
// or 1-arg (join). Tora's runtime helpers read at most the documented
// slots, so trailing operands typecheck-and-drop + lower-and-drop with
// the step()-side-effect path firing per ES eval-then-discard.

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

const base: number[] = [3, 1, 4];

// reverse — in-place, returns the same array; chain off it as we mutate
// the slice clone. Avoid printing the array directly (cross-runtime
// formatting gap); use .length + indexed access.
const r1: number[] = base.slice().reverse();
console.log("r1 length:", r1.length);
console.log("r1[0]:", r1[0]);
console.log("r1[1]:", r1[1]);
console.log("r1[2]:", r1[2]);

const r2: number[] = base.slice().reverse(step("extra"));
console.log("r2 length:", r2.length);
console.log("r2[0]:", r2[0]);
console.log("calls after r2:", calls);

const r3: number[] = base.slice().reverse(step("e1"), step(42), step(true));
console.log("r3 length:", r3.length);
console.log("r3[0]:", r3[0]);
console.log("calls after r3:", calls);

// toReversed — non-mutating, fresh array
const tr1: number[] = base.toReversed();
console.log("tr1 length:", tr1.length);
console.log("tr1[0]:", tr1[0]);

const tr2: number[] = base.toReversed(step("trail"));
console.log("tr2 length:", tr2.length);
console.log("tr2[2]:", tr2[2]);
console.log("calls after tr2:", calls);

const tr3: number[] = base.toReversed(step("t1"), step(7));
console.log("tr3[0]:", tr3[0]);
console.log("calls after tr3:", calls);

// join — 1-useful arg + trailing
console.log("--- join ---");
console.log(base.join(","));
console.log(base.join(",", step("trail")));
console.log("calls after join trail:", calls);
console.log(base.join("|", step("a"), step(5), step(false)));
console.log("calls after join multi:", calls);
console.log(base.join(undefined, step("u-trail")));
console.log("calls after join undef-sep trail:", calls);

// toString / toLocaleString — 0-useful + trailing
console.log("--- toString ---");
console.log(base.toString());
console.log(base.toString(step("ts-trail")));
console.log("calls after toString trail:", calls);

console.log("--- toLocaleString ---");
console.log(base.toLocaleString());
console.log(base.toLocaleString(step("tls-trail")));
console.log("calls final:", calls);
