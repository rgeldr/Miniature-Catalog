import assert from "node:assert/strict";
import { BROWSER_EXIT_MESSAGE, requestExit } from "../frontend/src/desktop-exit.ts";

const browser = [];
let closed = 0;
await requestExit({
  inDesktop: false,
  say: (message) => browser.push(message),
  closeWindow: async () => {
    closed += 1;
  },
});
assert.deepEqual(browser, [BROWSER_EXIT_MESSAGE]);
assert.equal(closed, 0);

const desktop = [];
await requestExit({
  inDesktop: true,
  say: (message) => desktop.push(message),
  closeWindow: async () => {
    closed += 1;
  },
});
assert.deepEqual(desktop, []);
assert.equal(closed, 1);

await assert.rejects(
  requestExit({
    inDesktop: true,
    say: () => {
      throw new Error("browser message should not run");
    },
    closeWindow: async () => {
      throw new Error("window close denied");
    },
  }),
  /window close denied/,
);

console.log("exit.test.mjs ok");
