// A string index read past the end is undefined (a string-view slot
// holding its own undefined cell), and stays undefined when it is
// stored into an any[] by index assignment, push or unshift.
let s = "abc";
let o: any[] = [];
o[0] = s[5];
o.push(s[7]);
o.push(s[1]);
o.unshift(s[9]);
console.log(typeof o[0], typeof o[1], typeof o[2], o[3], o.length);
console.log(o[1] === undefined, o.indexOf(undefined), o.join("|"));
