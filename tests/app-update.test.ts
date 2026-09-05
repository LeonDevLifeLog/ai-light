import assert from "node:assert/strict";
import test from "node:test";
import { compareVersions } from "../src/lib/app-update.ts";

test("compares stable semantic versions", () => {
  assert.equal(compareVersions("0.5.4", "0.5.3"), 1);
  assert.equal(compareVersions("v0.5.3", "0.5.3"), 0);
  assert.equal(compareVersions("0.5.2", "0.5.3"), -1);
});

test("compares every numeric component", () => {
  assert.equal(compareVersions("0.10.0", "0.9.9"), 1);
  assert.equal(compareVersions("1.0", "1.0.0"), 0);
});
