// Two Member reads whose kernels build their answer fresh — a symbol's
// description (§20.4.3.2) and a function's name (§20.2.4.2) — and
// therefore hand the reader a reference to give back. A Member
// expression is a borrow shape by default, so a read that transfers
// has to say so; when it does not, the consumer takes its own count
// and nobody ever releases the kernel's. That leaks one cell per read,
// and it stayed hidden because a description spelled as a literal is a
// static cell whose refcount traffic is a no-op — only an answer that
// reaches the heap can show the missing release.
//
// What is asserted here is the OTHER direction: that saying "this
// transfers" did not turn the leak into a use-after-free. Every value
// below is read out of something that then goes away, or is read twice
// and used after both reads.

// The description outlives the symbol that carried it.
function describe(i: number): string {
  const s = Symbol("live-" + i);
  return s.description!;
}
const kept: string[] = [];
for (let i = 0; i < 4; i++) kept.push(describe(i));

// Enough churn in between that a freed cell would have been reused.
for (let i = 0; i < 2000; i++) {
  const junk = Symbol("junk-" + i);
  if (junk.description!.length === 0) console.log("unreachable");
}
console.log(kept.join(","));

// Two reads of one description are independent values.
const shared = Symbol("shared");
const d1 = shared.description;
const d2 = shared.description;
console.log(d1, d2, d1 === d2, (d1! + "!").length, d2!.toUpperCase());

// No description at all is undefined, not the empty string.
const bare = Symbol();
console.log(bare.description, typeof bare.description);

// The Any lane answers the same thing.
const viaAny: any = Symbol("viaAny");
console.log(viaAny.description, String(viaAny.description).length);

// A function's name survives the binding it was read through.
function nameOf(): string {
  const local = function inner(a: number, b: number): number {
    return a + b;
  };
  return local.name;
}
const names: string[] = [];
for (let i = 0; i < 4; i++) names.push(nameOf());
for (let i = 0; i < 2000; i++) {
  const junk = Symbol("more-" + i);
  if (junk.description === undefined) console.log("unreachable");
}
console.log(names.join(","));

function named(a: number, b: number): number {
  return a + b;
}
const n1 = named.name;
const n2 = named.name;
console.log(n1, n2, n1 === n2, named.length, (n1 + n2).length);

// An anonymous shape reads the spec's empty name, not a null.
const anon = function (): number {
  return 1;
};
console.log(JSON.stringify(anon.name), anon.name.length);
