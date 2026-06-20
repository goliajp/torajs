// S296 — Map.{has,get,delete} + Set.{has,delete}(...trailing) trailing-
// arg ignore per ES §24.1.3.{3,4,6} (Map) / §24.2.3.{4,7} (Set). Spec
// reads only the 1 useful key/value slot; tora's ssa_lower arms only
// lowered args[0] via lower_to_tag_value and ignored args[1..] (S264
// comments asserted but never enforced). Probe shows calls=0 vs bun's
// natural accumulation across 8 trailing sites.
//
// Patch lower-and-drops trailing in 5 arms (Map.has/Map.delete/Map.get
// + Set.has/Set.delete) so step()-style side-effect exprs fire per
// ES eval-then-discard (S272 idiom).

let calls = 0;
function step<T>(v: T): T {
    calls = calls + 1;
    return v;
}

const m: Map<string, number> = new Map();
m.set("a", 1);
m.set("b", 2);

console.log("m.has bare:", m.has("a"));
console.log("m.has trail:", m.has("a", step("mh1")));
console.log("calls:", calls);
console.log("m.has multi:", m.has("a", step("mh2"), step(42), step(true)));
console.log("calls:", calls);
console.log("m.get bare:", m.get("a"));
console.log("m.get trail:", m.get("a", step("mg1")));
console.log("calls:", calls);
console.log("m.get multi:", m.get("b", step("mg2"), step(99)));
console.log("calls:", calls);
console.log("m.delete trail:", m.delete("a", step("md1")));
console.log("m.delete multi:", m.delete("b", step("md2"), step("md3")));
console.log("calls:", calls);

const s: Set<number> = new Set();
s.add(1);
s.add(2);
console.log("s.has bare:", s.has(1));
console.log("s.has trail:", s.has(1, step("sh1")));
console.log("calls:", calls);
console.log("s.has multi:", s.has(2, step("sh2"), step(11), step("z")));
console.log("calls:", calls);
console.log("s.delete trail:", s.delete(1, step("sd1")));
console.log("s.delete multi:", s.delete(2, step("sd2"), step(42)));
console.log("calls final:", calls);
