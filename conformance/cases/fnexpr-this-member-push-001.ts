// The function-expression family's push arm asks the same question the
// store position of the very same slot asks: does a value put here come
// back out through a receiver-honouring channel? It used to answer that
// by reading an `any[]` ANNOTATION, so a member-hop receiver was a loud
// reject. The annotation is not what decides — a root whose properties
// already live in the any world makes the container an any-world one
// whatever its field was declared as.
const f = function (a: any) {
  const th = this as any;
  return a + "/" + (th === undefined ? "undef" : "held");
};

const bag: any = {};
bag.fns = [];
bag.fns.push(f);
console.log(bag.fns[0](5));

const bag2: any = {};
bag2.fns = [];
bag2.fns.unshift(f);
console.log(bag2.fns[0](6));

// A keyed hop whose own root is a member hop.
const rows: any = {};
rows.grid = [[]];
rows.grid[0].push(f);
console.log(rows.grid[0][0](7));

// The annotated-binding receiver this census was built for is
// untouched.
const z: any[] = [];
z.push(f);
console.log(z[0](8));

// A this-free function expression keeps its own answer.
const q = function (a: number) {
  return a * 2;
};
const bag3: any = {};
bag3.fns = [];
bag3.fns.push(q);
console.log(bag3.fns[0](3));
