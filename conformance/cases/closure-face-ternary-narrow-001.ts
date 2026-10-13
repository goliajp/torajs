// A closure whose return is a choice that tests its optional parameter
// against undefined or null reads the parameter narrowed in the branch
// the test rules the nullish value out of, as the checker does: each
// returns a number.
function mk(k: number) { return (y?: number) => (y === undefined ? 5 : y) + k }
function mkFlip(k: number) { return (y?: number) => (undefined !== y ? y : 7) * k }
function mkLoose(k: number) { return (y?: number) => (y == null ? 1 : y) - k }
function mkNe(k: number) { return (y?: number) => (y != undefined ? y : 2) + k }
const a = mk(1), b = mkFlip(2), c = mkLoose(3), d = mkNe(4);
console.log(a(), a(2), b(), b(3), c(), c(10), d(), d(1));
const xs = [mk(10), mk(20)];
console.log(xs.map((f) => f(1)).join(","), xs[0]() + xs[1](2));
// a branch that reads another binding keeps the join
function mkOther(k: number) {
  return (y?: number, z?: number) => (y === undefined ? 5 : z);
}
console.log(mkOther(1)(undefined, 3), mkOther(1)(2, 3), mkOther(1)(2));
