// An initializer is the binding's first assignment, and control-flow
// analysis has always read it that way. tr narrowed at the second and
// not at the first, which stayed invisible for as long as nothing
// rewrote assignments into initializers — and then
// `desugar_uninit_let` did exactly that. It splices a follow-up write
// back onto its declaration, moving that assignment out from under
// the only hook that would have narrowed the binding.
//
// So two spellings of one program disagreed: `let c: C; log(c); c =
// new C(); c.pub` ran, because the read made the splice decline and
// left a statement to narrow on, while `let c: C | undefined; c = new
// C(); c.pub` was refused. The difference is not in the program.

class C {
  pub = 1;
  readonly ro = 2;
  m(): number {
    return this.pub * 10;
  }
}

// The spelling the splice resolves.
let c: C | undefined;
c = new C();
console.log("class:", c.pub, c.ro, c.m());

// The hand-written initializer says the same thing.
const d: C | undefined = new C();
console.log("init:", d.pub, d.m());

// Scalars ride their boxed lane through the narrow.
let x: number | undefined = 3;
console.log("num:", x + 1, x * 2, [x, 1][0]);

let t: boolean | null = true;
console.log("bool:", [t, false][0]);

// Pointer-shaped inners narrow to themselves.
let s: string | null = "hi";
console.log("str:", s.length, s + "!");

type O = { x: number };
let o: O | null = { x: 5 };
console.log("struct:", o.x);

const nums: number[] | null = [1, 2, 3];
console.log("arr:", nums.length, nums[1]);

// A declaration with no initializer keeps its union and keeps
// reading undefined — `Uninit` is one of the values the shared hook
// refuses to narrow on.
let u: number | undefined;
console.log("uninit:", u);

// A nullish initializer does not narrow either.
let v: string | null = null;
console.log("nullish:", v);
v = "x";
console.log("nullish after:", v);

// The narrow never shrinks what may be assigned: the declared union
// is what later assignments are checked against.
let w: number | null = 7;
console.log("w:", w + 1);
w = null;
console.log("w after:", w);

// A narrow does not outlive its straight-line segment: the loop body
// sees the union, which is what the guard inside it is for.
let k: string | null = "kk";
for (let i = 0; i < 2; i++) {
  if (k !== null) {
    console.log("loop:", i, k.length);
  }
}

// A handler inferred from the declaration is admitted against the
// narrowed value.
const p: string | null = "pp";
Promise.resolve(p).then((r) => {
  console.log("then:", r);
});
