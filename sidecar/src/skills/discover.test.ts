// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

import { discoverSkills, parseSkillFile } from "./discover.ts";

async function scaffold(): Promise<{
  cleanup: () => Promise<void>;
  userRoot: string;
  projectRoot: string;
  cwd: string;
}> {
  const base = await mkdtemp(path.join(tmpdir(), "cusa-skills-"));
  const userRoot = path.join(base, "user", ".cursor", "skills");
  const cwd = path.join(base, "project");
  const projectRoot = path.join(cwd, ".cursor", "skills");
  await mkdir(userRoot, { recursive: true });
  await mkdir(projectRoot, { recursive: true });
  return {
    userRoot,
    projectRoot,
    cwd,
    cleanup: () => rm(base, { recursive: true, force: true }),
  };
}

// ---- SPEC-030 -----------------------------------------------------------

test("SPEC-030: discoverSkills walks user and project roots recursively", async () => {
  const s = await scaffold();
  try {
    // user/.cursor/skills/frontend/react/SKILL.md
    await mkdir(path.join(s.userRoot, "frontend", "react"), {
      recursive: true,
    });
    await writeFile(
      path.join(s.userRoot, "frontend", "react", "SKILL.md"),
      "---\nname: react\ndescription: React hooks\n---\nBody R",
      "utf8",
    );
    // project/.cursor/skills/local/only/SKILL.md
    await mkdir(path.join(s.projectRoot, "local", "only"), {
      recursive: true,
    });
    await writeFile(
      path.join(s.projectRoot, "local", "only", "SKILL.md"),
      "---\nname: local-only\ndescription: proj-only\n---\nBody L",
      "utf8",
    );

    const out = await discoverSkills({
      cwd: s.cwd,
      userRoot: s.userRoot,
      projectRoot: s.projectRoot,
    });
    const ids = out.skills.map((x) => x.id).sort();
    assert.deepEqual(ids, ["frontend/react", "local/only"]);
  } finally {
    await s.cleanup();
  }
});

test("SPEC-030: project skill wins over same-id user skill", async () => {
  const s = await scaffold();
  try {
    await mkdir(path.join(s.userRoot, "shared"), { recursive: true });
    await writeFile(
      path.join(s.userRoot, "shared", "SKILL.md"),
      "---\nname: user-shared\n---\nUser body",
      "utf8",
    );
    await mkdir(path.join(s.projectRoot, "shared"), { recursive: true });
    await writeFile(
      path.join(s.projectRoot, "shared", "SKILL.md"),
      "---\nname: project-shared\n---\nProject body",
      "utf8",
    );

    const out = await discoverSkills({
      cwd: s.cwd,
      userRoot: s.userRoot,
      projectRoot: s.projectRoot,
    });
    const shared = out.skills.find((x) => x.id === "shared");
    assert.ok(shared, "expected shared skill");
    assert.equal(shared!.source, "project");
    assert.equal(shared!.name, "project-shared");
  } finally {
    await s.cleanup();
  }
});

// ---- SPEC-031 -----------------------------------------------------------

test("SPEC-031: parseSkillFile returns name + description + body", () => {
  const p = parseSkillFile(
    "---\nname: my-skill\ndescription: does things\n---\nHello body",
  );
  assert.deepEqual(p, {
    name: "my-skill",
    description: "does things",
    body: "Hello body",
  });
});

test("SPEC-031: parseSkillFile tolerates quoted values", () => {
  const p = parseSkillFile(
    '---\nname: "quoted"\ndescription: \'also-quoted\'\n---\nB',
  );
  assert.equal(p?.name, "quoted");
  assert.equal(p?.description, "also-quoted");
});

test("SPEC-031: parseSkillFile returns null when frontmatter is missing", () => {
  assert.equal(parseSkillFile("Just body text, no frontmatter."), null);
});

test("SPEC-031: parseSkillFile returns null when frontmatter is malformed YAML", () => {
  // Nested YAML lines are deliberately not supported (spec: any deeper YAML → skip).
  const p = parseSkillFile("---\nname: ok\nnested:\n  deep: yes\n---\nB");
  assert.equal(p, null);
});

test("SPEC-031: discovery emits a warning for missing/malformed frontmatter and continues", async () => {
  const s = await scaffold();
  try {
    // Good skill
    await mkdir(path.join(s.userRoot, "good"), { recursive: true });
    await writeFile(
      path.join(s.userRoot, "good", "SKILL.md"),
      "---\nname: good\ndescription: ok\n---\nB",
      "utf8",
    );
    // Bad skill (no frontmatter)
    await mkdir(path.join(s.userRoot, "bad"), { recursive: true });
    await writeFile(
      path.join(s.userRoot, "bad", "SKILL.md"),
      "Just some content without frontmatter",
      "utf8",
    );
    const warns: string[] = [];
    const out = await discoverSkills({
      cwd: s.cwd,
      userRoot: s.userRoot,
      projectRoot: s.projectRoot,
      onWarn: (m) => warns.push(m),
    });
    assert.equal(out.skills.length, 1);
    assert.equal(out.skills[0]!.id, "good");
    assert.ok(warns.some((w) => w.includes("frontmatter")));
  } finally {
    await s.cleanup();
  }
});
