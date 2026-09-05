// A fn parked in a container reached through a MEMBER hop keeps its
// receiver, the same way one reached through a keyed hop already did:
// what decides is the ROOT the chain starts from, not how the chain
// spells its way down from it.
function t(a: any) {
  const th = this as any;
  return a + "/" + (th === undefined ? "undef" : "held");
}

const bag: any = {};
bag.fns = [];
bag.fns.push(t);
console.log(bag.fns[0](5));

// The store spelling of the same slot.
const bag2: any = {};
bag2.fns = [];
bag2.fns[0] = t;
console.log(bag2.fns[0](5));

// A keyed hop whose own root is a member hop — refused until the
// chain peeled both spellings.
const rows: any = {};
rows.grid = [[]];
rows.grid[0].push(t);
console.log(rows.grid[0][0](6));

// Two member hops.
const deep: any = {};
deep.a = {};
deep.a.fns = [];
deep.a.fns.push(t);
console.log(deep.a.fns[0](9));

// The function-expression twin of the store position shares the one
// copy of this admission, so it moved with it.
const f = function (a: any) {
  const th = this as any;
  return a + "/" + (th === undefined ? "undef" : "held");
};
const bag3: any = {};
bag3.fns = [];
bag3.fns[0] = f;
console.log(bag3.fns[0](5));

// A binding root was already right and stays right.
const z: any[] = [];
z.push(t);
console.log(z[0](7));

// A plain call of the same declaration is untouched: the shell is
// minted per site, never on the declaration.
console.log(t(8));
