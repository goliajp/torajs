// A container literal built inside a function body carries the
// sentinel into its elements, and the for-of loop variable reads it
// back as undefined.
const zs: number[] = [1, 2, 3];

function outer(): void {
  const ys = [zs[9], 4];
  for (const x of ys) {
    console.log(typeof x, x === undefined);
  }
}

outer();
