// Nested-print substrate trunk Commit 8 acceptance — Tag::Promise
// typed-receiver console.log. SSA dispatcher Type::Promise →
// print_any → __torajs_print_anyv Tag::Promise branch →
// __torajs_promise_print reads Promise::state and emits the bun
// minimal form (no value column). Closes Promise typed-receiver
// wedge.
const p1: Promise<number> = Promise.resolve(42);
const p2: Promise<number> = Promise.reject(42);
p2.catch((e: number) => 0);
console.log(p1);
console.log(p2);
