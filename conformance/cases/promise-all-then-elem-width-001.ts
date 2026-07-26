// A `.then` handler taking an array of numbers has to agree with the
// promise's value on how wide those elements are.
//
// The wiring that joins a handler's parameter to the value slot only
// admitted the bare `number` spelling, so `(arr: number[]) => …` stayed
// out of it. The settled array then held integers while a parameter
// widened by anything else read them as f64.
//
// `await` was never affected — it reads the value slot directly rather
// than through a handler, which is why the two spellings of the same
// program disagreed.

// the scalar spelling this gate always admitted. It goes first because
// tr settles a `Promise.all` one microtask tick earlier than bun does,
// so an already-settled promise registered afterwards would interleave
// differently — a separate gap, noted in plan-state.
Promise.resolve(11).then((v: number) => {
  console.log("scalar", v);
});

const pn: Promise<number>[] = [Promise.resolve(1), Promise.resolve(2)];
Promise.all(pn).then((arr: number[]) => {
  // this seeds the parameter's element class F64 (find must be able to
  // answer undefined)
  arr.find((x: number): boolean => x > 0);
  console.log("all", arr.length, arr[0], arr[1]);
});

// a named handler, same shape
function show(arr: number[]): void {
  arr.find((x: number): boolean => x > 0);
  console.log("named", arr[0], arr[1]);
}
const pm: Promise<number>[] = [Promise.resolve(3), Promise.resolve(4)];
Promise.all(pm).then(show);

// widened by a fractional write instead of a method seed
const pw: Promise<number>[] = [Promise.resolve(5), Promise.resolve(6)];
Promise.all(pw).then((arr: number[]) => {
  arr[0] = 1.5;
  console.log("write", arr[0], arr[1]);
});

// the await spelling of the first one — already correct, kept as the
// pair that made the disagreement visible
async function viaAwait(): Promise<void> {
  const pa: Promise<number>[] = [Promise.resolve(7), Promise.resolve(8)];
  const arr: number[] = await Promise.all(pa);
  arr.find((x: number): boolean => x > 0);
  console.log("await", arr[0], arr[1]);
}
viaAwait();

// an untouched handler stays narrow and unaffected
const pq: Promise<number>[] = [Promise.resolve(9), Promise.resolve(10)];
Promise.all(pq).then((arr: number[]) => {
  console.log("narrow", arr[0], arr[1]);
});
