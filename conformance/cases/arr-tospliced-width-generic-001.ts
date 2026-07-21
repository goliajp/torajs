// toSpliced/with items feed the container width lattice — a promoted
// (any-demoted) receiver product materializes at a generic call
// boundary by its class width; an un-fed 0.5 item truncated to 0.
function rd<T>(xs: T[], i: number): T { return xs[i]; }
let arr = [0, 1, 2];
Object.defineProperty(arr, "0", { get() { arr.push(10); return 0; } });
Object.defineProperty(arr, "2", { get() { arr.push(11); return 2; } });
const p = arr.toSpliced(1, 0, 0.5);
console.log(p[0], p[1], p[2], p[3]);
console.log(rd(p, 0), rd(p, 1), rd(p, 2), rd(p, 3));
let brr = [0, 1, 2];
Object.defineProperty(brr, "0", { get() { return 0; } });
const w = brr.with(1, 2.5);
console.log(w[1], rd(w, 1));
