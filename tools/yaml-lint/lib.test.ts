import { describe, expect, test } from "bun:test";
import { lintText } from "./lib.ts";

function rules(findings: ReturnType<typeof lintText>): string[] {
  return findings.map((f) => f.rule);
}

describe("lintText", () => {
  test("clean file produces no findings", () => {
    const findings = lintText(
      "clean.yaml",
      "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: test\ndata:\n  key: value\n",
    );
    expect(findings).toEqual([]);
  });

  test("multi-document YAML parses without a false 'multiple documents' error", () => {
    const findings = lintText("multi.yaml", "a: 1\n---\nb: 2\n---\nc: 3\n");
    expect(findings).toEqual([]);
  });

  test("catches odd-space indentation", () => {
    const findings = lintText("bad-indent.yaml", "data:\n   badIndent: seven-spaces-above\n");
    expect(rules(findings)).toContain("indentation");
  });

  test("catches unquoted truthy scalars", () => {
    const findings = lintText("truthy.yaml", "enabled: yes\ndisabled: off\n");
    const truthy = findings.filter((f) => f.rule === "truthy");
    expect(truthy).toHaveLength(2);
  });

  test("does not flag a GitHub Actions 'on:' trigger key", () => {
    // `on:` is always followed by a nested mapping in real workflows, never a
    // bare scalar value on the same line — regression test for the removed
    // (and non-functional) check-keys exemption.
    const findings = lintText("workflow.yaml", "on:\n  push:\n    branches: [main]\n");
    expect(findings.filter((f) => f.rule === "truthy")).toEqual([]);
  });

  test("catches AWS access key id shape", () => {
    // Synthetic fixture, matches the AWS key *shape* on purpose to test the
    // detector — not a real credential.
    const findings = lintText("secret.yaml", 'password: "AKIAABCDEFGHIJKLMNOP"\n'); // gitleaks:allow
    expect(rules(findings)).toContain("secret-shape");
  });

  test("catches GitHub fine-grained PAT shape", () => {
    const findings = lintText(
      "secret2.yaml",
      `token: ${"github_pat_" + "A".repeat(30)}\n`,
    );
    expect(rules(findings)).toContain("secret-shape");
  });

  test("catches Stripe live key shape", () => {
    const findings = lintText("secret3.yaml", `key: ${"sk_live_" + "A".repeat(24)}\n`);
    expect(rules(findings)).toContain("secret-shape");
  });

  test("catches a genuine syntax error", () => {
    const findings = lintText("broken.yaml", "data:\n  broken: [unterminated\n");
    expect(rules(findings)).toContain("syntax");
  });

  test("skips indentation/truthy/comment checks inside block scalars", () => {
    const findings = lintText(
      "prose.yaml",
      "a: |\n  some text\n     seven-space indented prose, not YAML structure\nb: 2\n",
    );
    expect(findings).toEqual([]);
  });

  test("flags inline comments with fewer than 2 spaces before '#'", () => {
    const findings = lintText("comment.yaml", "key: value #comment\n");
    expect(rules(findings)).toContain("comments");
  });

  test("does not flag a full-line comment", () => {
    const findings = lintText("comment2.yaml", "# just a comment\nkey: value\n");
    expect(rules(findings)).toEqual([]);
  });
});
