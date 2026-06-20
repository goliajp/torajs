// S298 — Array.{fill,copyWithin}(...trailing) trailing-arg ignore per
// ES §23.1.3.{4,7}. Spec reads only the first 3 useful slots
// (value/target, start, end); tora's ssa_lower_str arms accepted the
// 4-arg shape via the `(args.len() >= 1 && args.len() <= 4)` /
// `(1..=4)` gates (S246 / S219) but never lowered args[3]. Probe
// shows calls=0 vs bun's natural accumulation at trailing site.
//
// Patch lower-and-drops args[3..] in 2 arms (fill 2299+, copyWithin
// 2050+) so step()-style side-effect exprs fire per ES eval-then-
// discard (S272 idiom). Helper Call ABI unchanged (recv + 3 indices).

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

const arr1: number[] = [1, 2, 3, 4, 5];
const f1: number[] = arr1.fill(0);
console.log("fill bare:", f1[0], f1[4], "calls:", calls);
const arr2: number[] = [1, 2, 3, 4, 5];
const f2: number[] = arr2.fill(0, 1, 4, step("fl1"));
console.log("fill trail:", f2[0], f2[1], f2[3], f2[4], "calls:", calls);
const arr3: string[] = ["a", "b", "c", "d"];
const f3: string[] = arr3.fill("z", 0, 2, step("fl2"));
console.log("fill str trail:", f3[0], f3[1], f3[2], f3[3], "calls:", calls);

const arr4: number[] = [1, 2, 3, 4, 5];
const cw1: number[] = arr4.copyWithin(0, 3);
console.log("cw bare:", cw1[0], cw1[1], cw1[4], "calls:", calls);
const arr5: number[] = [1, 2, 3, 4, 5];
const cw2: number[] = arr5.copyWithin(0, 1, 3, step("cw1"));
console.log("cw trail:", cw2[0], cw2[1], cw2[2], cw2[3], cw2[4], "calls:", calls);
const arr6: string[] = ["a", "b", "c", "d"];
const cw3: string[] = arr6.copyWithin(0, 2, 4, step("cw2"));
console.log("cw str trail:", cw3[0], cw3[1], cw3[2], cw3[3], "calls final:", calls);
