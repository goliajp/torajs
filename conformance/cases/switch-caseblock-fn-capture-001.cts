// §14.12.4 for a function in a switch clause that reads a binding of
// the function around it: it is initialized on entry to the CaseBlock
// like any other, so a clause the switch jumps to can call it, and it
// sees later writes to what it captured. Sloppy code (Annex B §B.3.3)
// also writes a var binding where the declaration runs.
//
// Oracle is node (`node <file>.cts`); bun answers ReferenceError for
// the cross-clause calls, which the spec does not allow.

function cap(x: number, k: number): number {
  switch (x) {
    case 0:
      function g() { return k * 2 }
      return 0;
    case 1:
      return g();
    default:
      return g() + 1;
  }
}
console.log(cap(1, 5), cap(9, 5), cap(0, 5));

// the binding stays live: the call sees the write made after entry
function live(x: number): number {
  let k = 1;
  switch (x) {
    case 0:
      function h() { return k }
      return -1;
    case 1:
      k = 5;
      return h();
  }
  return -2;
}
console.log(live(1));

// capturing peers in different clauses, one recursive
function peers(x: number, base: number): number {
  switch (x) {
    case 0:
      function even(n: number): boolean { return n === base ? true : odd(n - 1) }
      return 0;
    case 1:
      function odd(n: number): boolean { return n === base ? false : even(n - 1) }
      return even(10) ? 1 : 2;
  }
  return -1;
}
console.log(peers(1, 0), peers(1, 1));

function fact(x: number, one: number): number {
  switch (x) {
    case 0:
      function f(n: number): number { return n <= one ? one : n * f(n - 1) }
      return 0;
    default:
      return f(x);
  }
}
console.log(fact(5, 1));

// a capture of a clause's own let keeps the declaration in its clause
function local(x: number): string {
  switch (x) {
    case 0:
      let a = "a";
      function la() { return a }
      return la();
    case 1:
      return "one";
  }
  return "none";
}
console.log(local(0), local(1));

// the var binding is written only where the declaration runs
function varWrite(x: number, k: number): string {
  switch (x) {
    case 0:
      function v() { return k }
      break;
    case 1:
      break;
  }
  return typeof v;
}
console.log(varWrite(0, 1), varWrite(1, 1));
