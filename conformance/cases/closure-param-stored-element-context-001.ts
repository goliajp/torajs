// An arrow handed to an array mutator at an element position is not a
// callback being described — it is a value being stored, so the
// receiver's element type is that arrow's type, exactly as it is for
// one written as a literal element. With no context at all the
// parameter took its default and the call site still dispatched
// through the declared signature: `fs.push((n) => n + 1); fs[0](3)`
// answered -562949953421311, and the mutators that type-check their
// items said "got Function([Any], Any)" instead.

const fs: ((n: number) => number)[] = [];
fs.push((n) => n + 1);
console.log("push", fs[0](3));

const fs2: ((n: number) => number)[] = [];
fs2.push((n) => n + 1, (n) => n * 2);
console.log("push-two-args", fs2[0](3), fs2[1](3));

const fs3: ((n: number) => number)[] = [(n) => n + 1];
fs3.push((n) => n * 2);
console.log("push-onto-literal", fs3[0](3), fs3[1](3));

const fs4: ((n: number) => number)[] = [];
fs4.unshift((n) => n + 1);
console.log("unshift", fs4[0](3));

const fs5: ((n: number) => number)[] = [(n) => n];
fs5.fill((n) => n + 1);
console.log("fill", fs5[0](3));

const fs6: ((n: number) => number)[] = [(n) => n];
const fs6b = fs6.with(0, (n) => n + 1);
console.log("with", fs6b[0](3));

const fs7: ((n: number) => number)[] = [(n) => n];
fs7.splice(1, 0, (n) => n + 1);
console.log("splice-item", fs7[1](3));

// The parameter's own shape, in every spelling that reads one back.
const fs8: ((n: number) => number)[] = [];
fs8.push((n) => {
  console.log("  inside typeof", typeof n, "raw", n);
  return n + 1;
});
console.log("push-body", fs8[0](3));

const fs9: ((a: number, b: number) => number)[] = [];
fs9.push((a, b) => a * 100 + b);
console.log("push-two-params", fs9[0](3, 7));

const fs10: ((s: string) => string)[] = [];
fs10.push((s) => s + "!");
console.log("push-string", fs10[0]("hi"));

const fs11: ((n: number) => boolean)[] = [];
fs11.push((n) => n > 2);
console.log("push-boolean-ret", fs11[0](3), fs11[0](1));

const fs12: ((b: boolean) => boolean)[] = [];
fs12.push((b) => !b);
console.log("push-boolean-param", fs12[0](true));

// A named element type resolves through the alias, the same way an
// annotated binding's does.
type Op = (n: number) => number;
const fs13: Op[] = [];
fs13.push((n) => n + 1);
console.log("push-type-alias", fs13[0](3));

// A capturing arrow (the `Expr::Closure` post-lift shape) and one with
// no captures (a bare ident) both reach the hint.
const cap = 10;
const fs14: ((n: number) => number)[] = [];
fs14.push((n) => n + cap);
console.log("push-capture", fs14[0](3));

// The push does not have to sit next to the declaration.
const fs15: ((n: number) => number)[] = [];
function stash(): void {
  fs15.push((n) => n + 1);
}
stash();
console.log("push-from-fn-body", fs15[0](3));

// Shapes that must keep working: an author-annotated parameter still
// wins, a named function is untouched, and non-element positions of
// the same methods are read as they always were.
const fs16: ((n: number) => number)[] = [];
fs16.push((n: number) => n + 1);
console.log("push-annotated-param", fs16[0](3));

function nm(n: number): number {
  return n + 1;
}
const fs17: ((n: number) => number)[] = [];
fs17.push(nm);
console.log("push-named-fn", fs17[0](3));

const ns: number[] = [];
ns.push(1, 2);
ns.unshift(0);
console.log("push-numbers", ns[0], ns[1], ns[2], ns.length);

const ns2: number[] = [1, 2, 3];
ns2.fill(9, 1, 2);
console.log("fill-numbers", ns2[0], ns2[1], ns2[2]);

const ns3: number[] = [1, 2, 3];
console.log("with-numbers", ns3.with(1, 9)[1]);
console.log("splice-numbers", ns3.splice(1, 1)[0], ns3.length);

// Callback-bearing methods on the same receivers are unaffected.
const ns4: number[] = [3, 1, 2];
console.log("sort", ns4.sort((a, b) => a - b)[0]);
console.log("map", ns4.map((x) => x * 2)[2]);
console.log("filter", ns4.filter((x) => x > 1).length);
