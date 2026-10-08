// Values `JSON.parse` builds for an interface that class instances
// also stand behind: each parsed object answers the interface's
// members through its own layout's row.
interface Named {
  name: string;
}
class User {
  id: number = 0;
  name: string = "u";
}
const u: Named = new User();
const text = process.argv.length > 100 ? "" : '{"name":"ann"}';
const n: Named = JSON.parse(text);
console.log(u.name, n.name);
const xs: Named[] = JSON.parse('[{"name":"p"},{"name":"q"}]');
console.log(xs[1].name, xs.length);
const all: Named[] = [u, n, xs[0]];
let joined = "";
for (const v of all) joined += v.name;
console.log(joined);
