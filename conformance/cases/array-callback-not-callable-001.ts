// §7.2 middle tier at a callback slot. `[1,2].find({})` is a legal
// JavaScript program: §23.1.3.9 step 1 says IsCallable, and says
// throw a TypeError when it is false — at run time, after the
// receiver and the arguments have evaluated. tr refused it while
// compiling, which is the static-subset answer, not this language's.
//
// Nor is it the type-erasure answer. bun builds the argument, reaches
// the call, asks IsCallable and only then knows; tr knows while
// compiling that `{}` is not a function, and emits the throw with no
// check at all.
//
// The last two rows are the whole reason this reads the METHOD and
// not just the slot type. §23.1.3.x says throw; §27.2.5.4 says the
// opposite — a `then` handler that is not callable is replaced by
// the identity function. A predicate reading only the parameter type
// would have turned that refusal into a wrong throw.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
console.log("find object      ", attempt(() => [1, 2].find({})));
console.log("find string      ", attempt(() => [1, 2].find("x")));
console.log("find array       ", attempt(() => [1, 2].find([])));
console.log("find undefined   ", attempt(() => [1, 2].find(undefined)));
console.log("find null        ", attempt(() => [1, 2].find(null)));
console.log("find number      ", attempt(() => [1, 2].find(5)));
console.log("find bigint      ", attempt(() => [1, 2].find(1n)));
console.log("find regexp      ", attempt(() => [1, 2].find(/a/)));
console.log("findIndex object ", attempt(() => [1, 2].findIndex({})));
console.log("findLast object  ", attempt(() => [1, 2].findLast({})));
console.log("filter number    ", attempt(() => [1, 2].filter(5)));
console.log("map object       ", attempt(() => [1, 2].map({})));
console.log("forEach object   ", attempt(() => [1, 2].forEach({})));
console.log("every object     ", attempt(() => [1, 2].every({})));
console.log("some object      ", attempt(() => [1, 2].some({})));
console.log("flatMap object   ", attempt(() => [1, 2].flatMap({})));
console.log("reduce object    ", attempt(() => [1, 2].reduce({}, 0)));
console.log("sort null        ", attempt(() => [3, 1, 2].sort(null)));
console.log("sort object      ", attempt(() => [3, 1, 2].sort({})));
console.log("toSorted object  ", attempt(() => [3, 1, 2].toSorted({})));

// §13.3.6.1 evaluates the callee's base and then the arguments
// before the call, so both side effects survive the decided throw.
console.log("arg still runs   ", attempt(() => {
  let s = "";
  const g: any = () => {
    s += "g";
    return 5;
  };
  try {
    [1, 2].find(g());
  } catch (e) {
    s += "!";
  }
  return s;
}));
console.log("receiver runs    ", attempt(() => {
  let s = "";
  const r: any = () => {
    s += "r";
    return [1, 2];
  };
  try {
    r().find({});
  } catch (e) {
    s += "!";
  }
  return s;
}));

// §23.1.3.30 step 1 names `undefined` as "no comparator", so sort
// and toSorted alone do not throw on it.
console.log("sort undefined   ", attempt(() => [3, 1, 2].sort(undefined).join(",")));
console.log("toSorted undef   ", attempt(() => [3, 1, 2].toSorted(undefined).join(",")));
// And a real callback is untouched — this keeps its typed lane.
console.log("typed callback   ", [1, 2, 3].filter((x: number) => x > 1).join(","));
console.log("typed find       ", String([1, 2, 3].find((x: number) => x > 1)));

// The RECEIVER is checked too, not just the method name — but that
// half CANNOT be pinned here. A user object can own a `map` whose
// function never calls what it is handed
// (`({ map: (cb) => "no" }).map({})` answers "no" in every engine),
// and no spec clause says otherwise, so the right answer there is
// the ordinary type error — which refuses the whole program and so
// cannot share a fixture with runnable rows. The runner AOT-builds
// every case, so there is no refusal-fixture shape either. Keyed on
// the method name alone that pair answered `threw:TypeError`; the
// receiver check is what makes it a refusal again, and it is
// registered rather than pinned.
//
// Map and Set forEach DO say throw (§24.1.3.5 step 3 / §24.2.3.6
// step 3), so they are named on the receiver side.
const m = new Map<any, any>([[1, 2]]);
console.log("map forEach obj  ", attempt(() => m.forEach({})));
const st = new Set<any>([1]);
console.log("set forEach num  ", attempt(() => st.forEach(5)));
