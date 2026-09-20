// ES §23.1.3.7 steps 4/6 read fill's start / end through
// ToIntegerOrInfinity, which refuses no value. r678 widened the bound
// lane, but `Array<Any>` elements take a different lowering arm, and
// that arm fed the bound straight into an index normalizer that
// assumes an already-numeric operand -- so every non-numeric bound
// died as a compile-time refusal there.
//
// Two separate doors reach that arm: an empty array literal (no
// element type to go on), and any array that escapes into the `any`
// world. The sibling methods on the same receiver already passed --
// copyWithin / slice / indexOf do not write elements, so none of them
// has an Any-element arm to fall into.
console.log(JSON.stringify([].fill(1, null)));
console.log(JSON.stringify([].fill(1, undefined)));
console.log(JSON.stringify([].fill(1, 0, null)));
console.log(JSON.stringify([].fill(1, true)));

const e = [];
console.log(JSON.stringify(e.fill(1, null)));

// door 2: a typed array literal that escapes into `any`
function sink(x: any): any { return x; }
const escaped = [0, 0];
sink(escaped);
console.log(JSON.stringify(escaped.fill(1, null)));

const escaped2 = [0, 0, 0];
sink(escaped2);
console.log(JSON.stringify(escaped2.fill(7, 1, undefined)));

// a user valueOf that throws must reach the user, not a compile-time
// refusal -- the bound road runs it and checks for a pending throw.
const boom = { valueOf() { throw new Error("boom"); } };
try { [].fill(1, boom); console.log("no-throw"); } catch (err) { console.log("start", (err as Error).message); }
try { [].fill(1, 0, boom); console.log("no-throw"); } catch (err) { console.log("end", (err as Error).message); }

// controls that passed before this knife and must keep passing
const annotated: number[] = [];
try { annotated.fill(1, boom); console.log("no-throw"); } catch (err) { console.log("annotated", (err as Error).message); }
const nonEmpty = [0, 0];
try { nonEmpty.fill(1, boom); console.log("no-throw"); } catch (err) { console.log("nonEmpty", (err as Error).message); }
