// A module importing ITSELF whose default export is a class (test262 dynamic-import
// eval-export-dflt-cls-* shapes): the namespace's default field reads the class value
// through the dispatcher's named-fn body.
export default class { valueOf() { return 45; } }
export const x = 3;
import("./dyn-import-self-default-class-001.ts").then((imported: any) => {
  console.log(new imported.default().valueOf(), imported.x, typeof imported.default);
});
