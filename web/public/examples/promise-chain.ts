// 1000-deep .then chain. tr's Promise/microtask substrate handles
// it ~12x faster than bun-aot on Apple M4 Pro (1.24 ms vs 10.83 ms;
// see /bench).

function add1(v: number): number {
  return v + 1
}

let p = Promise.resolve(0)
for (let i = 0; i < 1000; i = i + 1) {
  p = p.then(add1)
}
console.log(await p) // 1000
