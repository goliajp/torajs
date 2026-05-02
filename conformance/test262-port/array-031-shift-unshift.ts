// Adapted from test262: built-ins/Array/prototype/shift +
// built-ins/Array/prototype/unshift. tr's variants both mutate the
// receiver. shift returns the popped first element (subset assumes
// non-empty); unshift returns void (return-value-of-len is the JS
// spec, but tr's API matches push for parser symmetry).
function check(): number {
  // Number array.
  let xs: number[] = [10, 20, 30, 40, 50];
  let h: number = xs.shift();
  if (h !== 10) { throw "#1: h"; }
  if (xs.length !== 4) { throw "#2: len"; }
  if (xs[0] !== 20) { throw "#3: head"; }
  if (xs[3] !== 50) { throw "#4: tail"; }

  xs.unshift(99);
  if (xs.length !== 5) { throw "#5: len"; }
  if (xs[0] !== 99) { throw "#6: head"; }
  if (xs[1] !== 20) { throw "#7"; }
  if (xs[4] !== 50) { throw "#8: tail"; }

  // String array — refcount path. shift transfers the first element's
  // ownership to the local; unshift takes ownership of its arg.
  let ys: string[] = ["alpha", "beta", "gamma"];
  let s1: string = ys.shift();
  if (s1 !== "alpha") { throw "#9: s1"; }
  if (ys.length !== 2) { throw "#10"; }
  if (ys[0] !== "beta") { throw "#11"; }

  ys.unshift("Q");
  if (ys.length !== 3) { throw "#12"; }
  if (ys[0] !== "Q") { throw "#13"; }
  if (ys[1] !== "beta") { throw "#14"; }
  if (ys[2] !== "gamma") { throw "#15"; }

  // Drain via shift loop.
  while (ys.length > 0) {
    ys.shift();
  }
  if (ys.length !== 0) { throw "#16: drained"; }

  // Unshift onto empty.
  ys.unshift("first");
  if (ys.length !== 1) { throw "#17"; }
  if (ys[0] !== "first") { throw "#18"; }
  return 0;
}
console.log(check());
