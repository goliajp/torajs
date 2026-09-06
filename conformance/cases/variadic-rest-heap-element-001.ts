// A rest parameter whose element type is REFCOUNTED had no boxed
// dual entry, so calling the closure that declares it died at "value
// is not a function". `const g = (...xs: string[]) => …` failed for
// any body and any arity, while the same function written as a
// declaration worked, and `number[]` / `boolean[]` / `any[]` rests
// worked — the element type alone decided it.
//
// The adapter reads its element kind from a private copy of the
// mapping `arr_kind_chain_of` owns, and that copy stopped at the
// three scalar kinds. The conversion kernel had handled heap
// elements all along, materializing a ShortStr immediate rather than
// refusing it. Nested typed arrays stay out on purpose: their kind
// is a chain and the kernel takes one level.

const join = (...xs: string[]): string => xs.join("-");
const count = (...xs: string[]): number => xs.length;
const first = (...xs: string[]): string => xs[0];
const walk = (...xs: string[]): string => {
  let s = "";
  for (const x of xs) {
    s += x;
  }
  return s;
};
console.log(join("a", "b"), count("a", "b", "c"), first("a", "b"), walk("a", "b"));
console.log(join(), count(), walk());

// short and long strings through the same tail — a short one is an
// immediate the conversion has to materialize, a long one is a cell
// it has to take a share of
const short = "x";
const long = "yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy";
console.log(join(short, long, short + long));

// a fixed prefix ahead of a heap tail
const tagged = (n: number, ...xs: string[]): string => n + xs.join("-");
console.log(tagged(1, "a", "b"), tagged(2));

// class instances are refcounted elements too
class C {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}
const sum = (...cs: C[]): number => {
  let t = 0;
  for (const c of cs) {
    t += c.v;
  }
  return t;
};
console.log(sum(new C(5), new C(6)), sum());

// called from inside a fn, and held as any
function fromFn(): string {
  return join("p", "q");
}
const asAny: any = join;
console.log(fromFn(), asAny("m", "n"));

// repeated calls: each tail takes its own shares and releases them
let total = 0;
for (let i = 0; i < 5; i++) {
  total += count("a" + i, "b");
}
console.log(total);

// the scalar element kinds, unchanged
const nums = (...xs: number[]): number => xs.length;
const bools = (...xs: boolean[]): number => xs.length;
console.log(nums(1, 2, 3), bools(true, false));
