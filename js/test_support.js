// test_support.js - Test doubles for module objects.
// A module's exported object is shared by every module that imports it, so a
// test swaps its members in place instead of rebinding a global name.

const originals = new Map();

/**
 * Replace every member of `target` with the members of `replacement`.
 * @param {object} target
 * @param {object} replacement
 */
function substitute(target, replacement) {
  const members = Object.getOwnPropertyDescriptors(replacement);
  if (!originals.has(target)) {
    originals.set(target, Object.getOwnPropertyDescriptors(target));
  }
  for (const key of Reflect.ownKeys(target)) delete target[key];
  Object.defineProperties(target, members);
}

/**
 * Put back the members `target` had before its first substitution.
 * @param {object} target
 */
function restore(target) {
  const members = originals.get(target);
  if (!members) return;
  for (const key of Reflect.ownKeys(target)) delete target[key];
  Object.defineProperties(target, members);
  originals.delete(target);
}

function restoreAll() {
  for (const target of [...originals.keys()]) restore(target);
}

export { substitute, restore, restoreAll };
