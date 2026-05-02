// Adapted from test262: built-ins/Array/prototype/pop. tr's `pop`
// is mutating, returns the removed last element, decrements length.
// Subset assumes a non-empty receiver (no `T | undefined` since tr
// lacks union types).
function check(): number {
  // Number array.
  let xs: number[] = [10, 20, 30, 40, 50];
  if (xs.length !== 5) { throw "#1: len"; }
  let v: number = xs.pop();
  if (v !== 50) { throw "#2: v"; }
  if (xs.length !== 4) { throw "#3: len"; }
  if (xs[3] !== 40) { throw "#4: tail"; }

  // Pop until empty (last pop may stress the slot/len-edge case).
  while (xs.length > 0) {
    xs.pop();
  }
  if (xs.length !== 0) { throw "#5: empty"; }

  // String array — refcount path. Each popped Str transfers ownership
  // to the local; the implicit drop at scope end frees it.
  let ys: string[] = ["alpha", "beta", "gamma"];
  let s1: string = ys.pop();
  if (s1 !== "gamma") { throw "#6: s1"; }
  if (ys.length !== 2) { throw "#7: ys.len"; }
  if (ys[0] !== "alpha") { throw "#8: ys[0]"; }
  if (ys[1] !== "beta") { throw "#9: ys[1]"; }

  // Push after pop must reuse the slot (no orphan from earlier push).
  ys.push("Q");
  if (ys[2] !== "Q") { throw "#10: push-after-pop"; }
  if (ys.length !== 3) { throw "#11"; }
  return 0;
}
console.log(check());
