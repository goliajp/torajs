// `xs.pop()` now accepts a const-global Array<T> receiver
// (previously only mutable local Array<T> bindings). Pop is
// read-and-mutate-in-place — len decrements without realloc — so
// the in-place mutation persists on the global heap object even
// without a write-back of the array pointer through a slot.
// `const xs: T[]` at top-level becomes a K.3 global; pre-fix,
// `xs.pop()` panicked with "unsupported member call shape: pop"
// because the dispatch only matched `self.locals.get(name)`.

const ints: number[] = [10, 20, 30, 40];
console.log(ints.pop());       // 40
console.log(ints.pop());       // 30
console.log(ints.length);      // 2
console.log(ints[0]);          // 10
console.log(ints[1]);          // 20

const strs: string[] = ["alpha", "beta", "gamma"];
console.log(strs.pop());       // gamma
console.log(strs.length);      // 2

// Stack pattern: push then pop on a mutable local.
let stack: number[] = [];
stack.push(1);
stack.push(2);
stack.push(3);
console.log(stack.pop());      // 3
console.log(stack.pop());      // 2
console.log(stack.length);     // 1

// Pop on a fn-local Array<string>.
function lastWord(words: string[]): string {
  return words.pop()!;
}
console.log(lastWord(["one", "two", "three"]));  // three
