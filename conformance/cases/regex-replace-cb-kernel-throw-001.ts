// §22.2.6.11 step 14.j — a replace callback that throws ends the walk
// there, and the throw reaches the `try` the program wrote around the
// replace.
//
// A throw the callback raised ITSELF was always caught. One the KERNEL
// raised inside it was not: an unmatched capture group binds
// `string | undefined`, calling a method on that trips the guard, and
// the typed-callback lane had no post-call throw check (neither
// `any`-replacement nor `replaceAll`, the two spellings that did).
// Three things went wrong at once — the remaining matches still ran
// the callback, the result of the throwing replace was spliced and
// returned, and the throw surfaced after the `try` had gone out of
// scope, ending the process uncaught.

function line(tag: string, f: () => unknown) {
  try {
    console.log(tag, f());
  } catch (e) {
    console.log(tag, "THREW", (e as Error).name);
  }
}

// The callback's own throw — the leg that always worked.
line("explicit", () =>
  "x".replace(/(y)?x/, () => {
    throw new TypeError("mine");
  }),
);

// The kernel's guard on an unmatched capture group.
line("kernel", () => "x".replace(/(y)?x/, (m, p1) => p1.toUpperCase()));

// The walk stops at the first throwing match, so the callback runs
// once even though the global pattern matches twice.
let calls = 0;
line("global", () =>
  "axbxc".replace(/(y)?x/g, (m, p1) => {
    calls++;
    return p1.toUpperCase();
  }),
);
console.log("calls", calls);

// A matched group is a plain string and the walk completes.
line("matched", () => "yx".replace(/(y)?x/, (m, p1) => p1.toUpperCase()));

// replaceAll's own check still works, and so does its callback's.
line("all-kernel", () => "axbx".replaceAll(/(y)?x/g, (m, p1) => p1.toUpperCase()));
