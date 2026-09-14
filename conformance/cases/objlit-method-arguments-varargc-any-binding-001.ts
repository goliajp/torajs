// An object-literal method that touches `arguments`, reached by direct
// member calls off a `: any` binding with more than one argc. The
// static objlit face needs a single argc vote and the boxed-only face
// refused on the member spelling, so the body had no argv face at all.
let seen: any[] = [];
let it: any = {
  keep() { seen.push(arguments); },
  store() { const a = arguments; seen.push(a); },
  slot() { seen[seen.length] = arguments; },
  len() { return arguments.length; },
  first(a: any) { return arguments.length + ":" + a; },
};
it.keep(1);
it.keep(2, 3);
it.store("x");
it.store("y", "z", "w");
it.slot();
it.slot(9, 8);
console.log(seen.length);
console.log(seen.map((x: any) => x.length).join(","));
console.log(seen[1][1], seen[3][2], seen[5][0]);
console.log(it.len(), it.len(1), it.len(1, 2, 3));
console.log(it.first(5), it.first(5, 6));

// the escape-store spelling that already worked keeps working
let args: any = null;
let holder: any = { m() { args = arguments; } };
holder.m(1);
holder.m(2, 3);
console.log(args.length, args[1]);
