import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { nameError, nameRules } from "../src/validation.ts";

test("resource names share the server's exact guidance and bounds", () => {
  const server = readFileSync(new URL("../../crates/core/src/validate.rs", import.meta.url), "utf8");
  assert.equal(nameRules.resource.message, /pub const NAME_RULE: &str = "([^"]+)"/.exec(server)[1]);
  for (const value of ["a", "0", "web-1", "a-", "a".repeat(63)]) {
    assert.equal(nameError(value), null, value);
  }
  for (const value of ["", "-web", "Web", "under_score", "has space", "tiếng", "a".repeat(64), "web\n"]) {
    assert.equal(nameError(value), nameRules.resource.message, value);
  }
});

test("optional names stay optional and Compose has its own character rules", () => {
  assert.equal(nameError("", "resource", false), null);
  assert.equal(nameError("Web_1.api", "compose"), null);
  assert.equal(nameError("Web_1.api"), nameRules.resource.message);
  for (const value of ["_web", ".web", "a".repeat(64)]) {
    assert.equal(nameError(value, "compose"), nameRules.compose.message);
  }
});

test("environment keys describe their different start character and limit", () => {
  for (const value of ["_", "DATABASE_URL", "A".repeat(128)]) {
    assert.equal(nameError(value, "environment"), null);
  }
  for (const value of ["", "1NAME", "has-dash", "A".repeat(129)]) {
    assert.equal(nameError(value, "environment"), nameRules.environment.message);
  }
});

test("every shared name rule has Vietnamese guidance", () => {
  const dictionary = readFileSync(new URL("../src/i18n/vi.ts", import.meta.url), "utf8");
  for (const rule of Object.values(nameRules)) {
    assert.ok(dictionary.includes(JSON.stringify(rule.message)));
  }
});
