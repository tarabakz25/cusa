// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import { test } from "node:test";
import assert from "node:assert/strict";

import type { DiscoveredSkill } from "./discover.ts";
import { buildSkillContext } from "./inject.ts";

function skill(id: string, body: string, name = id): DiscoveredSkill {
  return {
    id,
    name,
    description: `desc-${name}`,
    path: `/tmp/${id}/SKILL.md`,
    sizeBytes: body.length,
    source: "user",
    body,
  };
}

// ---- SPEC-033 -----------------------------------------------------------

test("SPEC-033: buildSkillContext produces the documented XML block", () => {
  const out = buildSkillContext([skill("foo", "BODY_A")]);
  assert.match(out, /^<skills>\n/);
  assert.match(out, /<\/skills>$/);
  assert.match(
    out,
    /<skill name="foo"><description>desc-foo<\/description><body>BODY_A<\/body><\/skill>/,
  );
});

test("SPEC-033: empty enabled list returns empty string (no wrapper)", () => {
  assert.equal(buildSkillContext([]), "");
});

test("SPEC-033: XML-escapes name/description/body content", () => {
  const s = skill("x", "<tag>&value</tag>");
  s.name = "n<ame>";
  s.description = 'has "quote"';
  const out = buildSkillContext([s]);
  assert.ok(out.includes("n&lt;ame&gt;"));
  assert.ok(out.includes("has &quot;quote&quot;"));
  assert.ok(out.includes("&lt;tag&gt;&amp;value&lt;/tag&gt;"));
});

// ---- SPEC-034 -----------------------------------------------------------

test("SPEC-034: budget cap drops overflow skills and emits a warning", () => {
  const warns: string[] = [];
  // Each skill body ~5000 bytes → 2 fit under a 12 KiB budget.
  const big = "X".repeat(5000);
  const skills = [
    skill("aaa", big),
    skill("bbb", big),
    skill("ccc", big),
    skill("ddd", big),
  ];
  const out = buildSkillContext(skills, {
    budgetBytes: 12 * 1024,
    onWarn: (m) => warns.push(m),
  });
  const matches = out.match(/<skill name="/g) ?? [];
  assert.equal(
    matches.length,
    2,
    `expected 2 skill blocks under budget, got ${matches.length}`,
  );
  assert.ok(out.includes('name="aaa"'));
  assert.ok(!out.includes('name="ddd"'));
  assert.equal(warns.length, 1);
  assert.match(warns[0]!, /truncated/);
  assert.match(warns[0]!, /ddd/);
});

test("SPEC-034: fits everything under a generous budget", () => {
  const warns: string[] = [];
  const out = buildSkillContext(
    [skill("a", "AAA"), skill("b", "BBB")],
    { budgetBytes: 16 * 1024, onWarn: (m) => warns.push(m) },
  );
  assert.equal(warns.length, 0);
  assert.ok(out.includes("AAA"));
  assert.ok(out.includes("BBB"));
});
