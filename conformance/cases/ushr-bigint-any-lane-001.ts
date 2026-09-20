// §13.12 gives `>>>` no BigInt form, so a BigInt operand always ends
// in a TypeError — but WHEN is the question. Deciding it at compile
// time is only the same program when nothing observable happens
// first, and §13.15.3 runs ToPrimitive on both operands before the
// operator refuses them. An object's ToPrimitive is user code.
//
// So the refusal belongs after the lanes that route such a pair to
// the any kernel, not ahead of them: the kernel runs the coercions
// in spec order and raises whatever they raise.
function show(tag: string, f: () => unknown): void {
  try {
    console.log(tag, "ok", f());
  } catch (e: any) {
    console.log(tag, "throw", e.constructor.name);
  }
}

const n: any = 5;
show("any>>>bigLit", () => n >>> 1n);

const b: bigint = 1n;
show("any>>>typedBig", () => n >>> b);

// The object side reaches ToUint32 through `valueOf`, which is why
// the pair rides the any kernel rather than a typed one.
const o = { valueOf(): number { return 4; } };
show("obj>>>bigLit", () => o >>> 1n);

// And when that user code throws, ITS error is the one the program
// sees — not the TypeError the operator would have raised.
const tp = {
  [Symbol.toPrimitive](): number {
    throw new RangeError("mine");
  },
};
show("toPrimitive-first", () => tp >>> 0n);
