// The callback parameter of a HOF over a sentinel-carrying container
// is itself a sentinel source, so writing it into a second container
// taints that container's elements too.
const zs: number[] = [1, 2, 3];
const ys: number[] = [zs[9], 4];
const ws: number[] = [];

ys.forEach((x) => {
  ws.push(x);
});

ws.forEach((w) => {
  console.log(typeof w, w === undefined);
});
