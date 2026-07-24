// dynobj-degrade scope-correctness (rotation 203 chunk 1) — the
// defineProperty receiver set is keyed by DECLARATION site, not by
// name. Locks the three resolution faces of `crate::dynobj_degrade`:
//
// 1. a helper's local receiver degrades and the define lands (x=41)
// 2. an unrelated same-named top-level binding stays statically
//    typed (obj.a reads through the struct lane) — the rotation-200
//    passTotal-inflation shape, now collision-free
// 3. a free-name receiver inside a closure body (captured top-level
//    binding, no local decl) still degrades via the conservative
//    name fallback — a miss there would orphan the defined property
//    (silent wrong). Named-fn bodies can't observe this face: the
//    globals pre-registration never surfaces a degraded object
//    binding to named fns (pre-existing, tracked in plan-state L3b).

function helper() {
    let obj = {};
    Object.defineProperty(obj, "x", { value: 41 });
    console.log(obj.x);
}
helper();

let obj = { a: 7 };
console.log(obj.a);

let g = { b: 1 };
const poke = () => {
    Object.defineProperty(g, "c", { value: 9 });
};
poke();
console.log(g.c);
console.log(g.b);
