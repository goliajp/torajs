// S297 — Object.{keys,values,entries,getOwnPropertyNames} +
// Reflect.ownKeys (...trailing) trailing-arg ignore per ES
// §20.1.2.{5,17,22,23} / §28.1.11. Spec reads only the target obj
// (args[0]); tora's ssa_lower 3 reflection arms only lowered args[0]
// and dropped args[1..] silently (S255/S256/S258 comments asserted
// trailing dropped but the loop was never wired). Probe shows
// calls=0 vs bun's natural accumulation across 3 trailing sites.
//
// Patch lower-and-drops args[1..] in 3 arms (Object.keys/getOPN/
// Reflect.ownKeys 18915+, Object.values 18660+, Object.entries
// 19774+) so step()-style side-effect exprs fire per ES eval-then-
// discard (S272 idiom).

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

const o: {a: number, b: number, c: number} = {a: 1, b: 2, c: 3};

const k1: string[] = Object.keys(o);
console.log("keys bare:", k1.length, "calls:", calls);
const k2: string[] = Object.keys(o, step("k1"));
console.log("keys trail:", k2.length, "calls:", calls);
const k3: string[] = Object.keys(o, step("k2"), step(42), step(true));
console.log("keys multi:", k3.length, "calls:", calls);

const v1: number[] = Object.values(o);
console.log("values bare:", v1.length);
const v2: number[] = Object.values(o, step("v1"));
console.log("values trail:", v2.length, "calls:", calls);
const v3: number[] = Object.values(o, step("v2"), step("v3"));
console.log("values multi:", v3.length, "calls:", calls);

const e1 = Object.entries(o);
console.log("entries bare:", e1.length);
const e2 = Object.entries(o, step("e1"));
console.log("entries trail:", e2.length, "calls:", calls);
const e3 = Object.entries(o, step("e2"), step(99), step("z"));
console.log("entries multi:", e3.length, "calls:", calls);

const opn1: string[] = Object.getOwnPropertyNames(o);
console.log("getOPN bare:", opn1.length);
const opn2: string[] = Object.getOwnPropertyNames(o, step("opn1"), step(7));
console.log("getOPN trail:", opn2.length, "calls:", calls);

const rk1: string[] = Reflect.ownKeys(o);
console.log("ownKeys bare:", rk1.length);
const rk2: string[] = Reflect.ownKeys(o, step("rk1"), step("rk2"));
console.log("ownKeys trail:", rk2.length, "calls final:", calls);
