// §13.5.1 delete on an array index. The declaration is what decides
// whether the storage can say "no longer here", so a binding the
// program deletes out of widens to `any[]` — an unboxed number slot
// has no value that means absent.
let a = [1, 2, 3];
let d = delete a[1];
console.log(d);
console.log(a[1]);
console.log(a.length);
console.log(1 in a, 0 in a);
console.log(Object.keys(a).join(","));
console.log(JSON.stringify(a));
console.log(a.indexOf(undefined));

// A write revives the index as a plain data property.
a[1] = 9;
console.log(1 in a, a[1], Object.keys(a).join(","));

// An explicit annotation is the user's word and stays: this one is
// already `any[]`, so the delete is admitted the same way.
let b: any[] = ["x", "y"];
console.log(delete b[0], b[0], b.length, 0 in b);

// A binding nobody deletes from keeps its element type.
let c = [4, 5, 6];
console.log(c[0] + c[1] + c[2]);

// A binding `let_widen` already claims — reassigned across syntactic
// families — is left alone: that pass types it `any`, which is wider
// than `any[]` and admits the delete on its own. Annotating here would
// take it out of `let_widen`'s reach and refuse the reassign.
let x = [0];
delete x[0];
console.log(x.length);
x = { p: 1 } as any;
console.log(x.p);
