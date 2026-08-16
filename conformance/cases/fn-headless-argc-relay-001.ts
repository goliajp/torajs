// A head-less fn whose name escapes as a value keeps answering the
// true `arguments.length` through the `__forward_` relay, and must
// NOT be reshaped by the argv face.
//
// The relay's forwarding call carries its own RUNTIME argc into the
// callee's hidden slot, while the argv packer at a direct call site
// can only fill a buffer of the statically written argument count.
// Admitting such a fn to the argv face made the body materialize
// argv[0..argc] off the end of a shorter stack slab — 14 test262
// cases went from a wrong answer to SIGSEGV / SIGBUS, none of which
// the conformance gate could see (it compares stdout, and these
// bodies were already answering the wrong thing).
//
// This case guards the argc half of that link. The VALUE half stays
// on the declared-params approximation, which is not bun-equal and
// therefore cannot be pinned here — registered residue, plan-state
// L3b.

function counted(a: number) {
  return "n=" + arguments.length + " a=" + a;
}

// The value escape is what mints the relay.
const held: any = counted;
console.log(held(1, 2, 3));
console.log(held(4));
console.log(counted(5, 6));

// Reached through a container, too.
const bag: any[] = [counted];
console.log(bag[0](7, 8, 9));

// A second head-less counter with no escape at all still reads its
// own argc — the two tiers coexist in one program.
function direct(a: number) {
  return "d=" + arguments.length;
}
console.log(direct(1, 2));
console.log(direct(9));
