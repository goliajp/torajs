// `xs.shift()` extended to accept K.3 const-global Array<T>
// receivers (previously only mutable local Array<T> bindings).
// Like pop, shift mutates the array in place — it bumps the
// head_offset and decrements len without realloc — so the
// in-place mutation persists on the global without writeback.

const ints: number[] = [10, 20, 30, 40];
console.log(ints.shift());     // 10
console.log(ints.shift());     // 20
console.log(ints.length);      // 2
console.log(ints[0]);          // 30

const strs: string[] = ["alpha", "beta", "gamma"];
console.log(strs.shift());     // alpha
console.log(strs.length);      // 2
console.log(strs[0]);          // beta
