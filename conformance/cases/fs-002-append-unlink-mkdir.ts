// v0.3 #1.b: fs.appendFileSync / unlinkSync / mkdirSync.

import { writeFileSync, appendFileSync, readFileSync, existsSync, unlinkSync, mkdirSync } from "fs";

const tmp = "/tmp/torajs_conf_fs_b.txt";

// Append round-trip
writeFileSync(tmp, "hello");
appendFileSync(tmp, " world");
console.log(readFileSync(tmp).length);

appendFileSync(tmp, "!");
console.log(readFileSync(tmp).length);

// Unlink + existsSync round-trip
unlinkSync(tmp);
console.log(existsSync(tmp));

// mkdirSync — create then verify exists. Unlink the dir at end via rmdir-equivalent
// (deferred — just create a unique dir and leave it; conformance runner is fine).
const tmpdir = "/tmp/torajs_conf_dir_" + (Math.floor(Math.random() * 100000)).toString();
if (!existsSync(tmpdir)) {
  mkdirSync(tmpdir);
}
console.log(existsSync(tmpdir));
