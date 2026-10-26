import assert from "node:assert/strict";
import { replaceOnce, replaceVersionNear } from "./version.mjs";

const lf = `{
  "name": "3d-file-library",
  "version": "26.10.2",
  "other": { "version": "9.9.9" }
}`;

const crlf = lf.replace(/\n/g, "\r\n");

const nextLf = replaceOnce(
  lf,
  `"name": "3d-file-library",\n  "version": "26.10.2"`,
  `"name": "3d-file-library",\n  "version": "26.10.3"`,
);
assert.match(nextLf, /"version": "26.10.3"/);
assert.match(nextLf, /"other": { "version": "9.9.9" }/);

const nextCrlf = replaceOnce(
  crlf,
  `"name": "3d-file-library",\n  "version": "26.10.2"`,
  `"name": "3d-file-library",\n  "version": "26.10.3"`,
);
assert.ok(nextCrlf.includes("\r\n"));
assert.match(nextCrlf, /"version": "26.10.3"/);

let threw = false;
try {
  replaceOnce(lf, `"name": "3d-file-library",\n  "version": "26.10.9"`, `"version": "26.10.3"`);
} catch {
  threw = true;
}
assert.equal(threw, true);

const withPrivate = `{
  "name": "3d-file-library",
  "private": true,
  "version": "26.10.4",
  "dependencies": { "left-pad": "26.10.4" }
}`;
const bumpedPrivate = replaceVersionNear(withPrivate, `"name": "3d-file-library"`, "26.10.4", "26.10.5");
assert.match(bumpedPrivate, /"private": true/);
assert.match(bumpedPrivate, /"version": "26.10.5"/);
assert.match(bumpedPrivate, /"left-pad": "26.10.4"/);

console.log("version.test.mjs ok");
