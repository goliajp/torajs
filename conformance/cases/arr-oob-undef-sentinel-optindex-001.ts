// `xs?.[i]` takes the same checked-index exit as `xs[i]` — the `?.`
// guards only the receiver — so an out-of-range read answers the same
// `undefined`. The hit path boxed the element with the encoder that
// takes no expression id, which is the only one that does not ask
// whether the value may be the sentinel, so it arrived in the any
// world as a NaN carrying our payload.
const zs: number[] = [1, 2, 3];
console.log(zs?.[9], typeof zs?.[9], zs?.[9] === undefined);

const a1 = zs?.[9];
console.log(a1, typeof a1, a1 === undefined);

const o: { xs: number[] } = { xs: [1, 2, 3] };
console.log(o.xs?.[9], typeof o.xs?.[9]);

// through the value-transparent wrappers the predicate already names
console.log(true ? zs?.[9] : 0);
console.log((0, zs?.[9]));

// in-range reads on the same shapes stay ordinary numbers
console.log(zs?.[1], o.xs?.[0], typeof zs?.[1]);

// the pointer-shaped families were already right; they stay right
const ss: string[] = ["a", "b"];
console.log(ss?.[9], typeof ss?.[9], ss?.[1]);
const s2 = "hi";
console.log(s2?.[9], s2?.[1]);
const nn: number[] | null = null;
console.log(nn?.[0]);
