/**
 * yaml-lint core checks — split out from index.ts so they're unit-testable
 * (bun:test) independent of file I/O / glob scanning.
 */

import { LineCounter, parseAllDocuments, visit, type Document } from "yaml";

// Reused verbatim from infra-code's .github/scripts/validate-docs.ts
// SECRET_PATTERNS, per this tool's own design doc ("Reuse patterns from
// validate-docs.ts") — do not fork this list, keep it in sync instead.
export const SECRET_PATTERNS: Array<{ name: string; re: RegExp }> = [
  { name: "private key block", re: /-----BEGIN (?:[A-Z0-9]+ )?PRIVATE KEY-----/ },
  { name: "GitHub personal access token", re: /ghp_[A-Za-z0-9]{36,255}/ },
  { name: "GitHub fine-grained PAT", re: /github_pat_[A-Za-z0-9]{22,255}/ },
  { name: "Stripe live key", re: /sk_live_[A-Za-z0-9]{20,}/ },
  { name: "AWS access key id", re: /AKIA[0-9A-Z]{16}/ },
];

const TRUTHY_VALUE = /^(?:yes|no|on|off)$/i;
const BLOCK_SCALAR_TYPES = new Set(["BLOCK_LITERAL", "BLOCK_FOLDED"]);

export type Severity = "error" | "warning";

export interface Finding {
  file: string;
  line: number;
  col: number;
  severity: Severity;
  rule: string;
  message: string;
}

/** Lines 1-indexed inside `|`/`>` block scalars — content there isn't structural YAML. */
export function blockScalarLines(docs: Document[], lineCounter: LineCounter): Set<number> {
  const skip = new Set<number>();
  for (const doc of docs) {
    visit(doc, {
      Scalar(_key, node) {
        if (!node.range || !BLOCK_SCALAR_TYPES.has(node.type ?? "")) return;
        const start = lineCounter.linePos(node.range[0]);
        const end = lineCounter.linePos(node.range[1]);
        // Skip the line after the `key: |` marker through the scalar's end —
        // the marker line itself stays subject to normal checks.
        for (let l = start.line + 1; l <= end.line; l++) skip.add(l);
      },
    });
  }
  return skip;
}

export function checkTabs(file: string, lines: string[], skip: Set<number>): Finding[] {
  const findings: Finding[] = [];
  lines.forEach((line, i) => {
    if (skip.has(i + 1)) return;
    const leading = line.match(/^[ \t]*/)?.[0] ?? "";
    if (leading.includes("\t")) {
      findings.push({
        file,
        line: i + 1,
        col: leading.indexOf("\t") + 1,
        severity: "error",
        rule: "no-tabs",
        message: "tabs are not allowed for indentation — use spaces",
      });
    }
  });
  return findings;
}

export function checkIndentSpacing(file: string, lines: string[], skip: Set<number>): Finding[] {
  const findings: Finding[] = [];
  lines.forEach((line, i) => {
    if (skip.has(i + 1)) return;
    if (/^\s*#/.test(line) || line.trim() === "") return;
    const leading = line.match(/^ */)?.[0].length ?? 0;
    if (leading % 2 !== 0) {
      findings.push({
        file,
        line: i + 1,
        col: leading + 1,
        severity: "error",
        rule: "indentation",
        message: `indentation must be a multiple of 2 spaces (found ${leading})`,
      });
    }
  });
  return findings;
}

export function checkTruthy(file: string, lines: string[], skip: Set<number>): Finding[] {
  const findings: Finding[] = [];
  lines.forEach((line, i) => {
    if (skip.has(i + 1)) return;
    const match = line.match(/^\s*(?:-\s*)?[A-Za-z0-9_.-]+:\s*([A-Za-z]+)\s*(#.*)?$/);
    if (!match) return;
    const value = match[1];
    if (TRUTHY_VALUE.test(value)) {
      findings.push({
        file,
        line: i + 1,
        col: line.indexOf(value) + 1,
        severity: "warning",
        rule: "truthy",
        message: `ambiguous scalar "${value}" — quote it or use true/false explicitly`,
      });
    }
  });
  return findings;
}

export function checkCommentSpacing(file: string, lines: string[], skip: Set<number>): Finding[] {
  const findings: Finding[] = [];
  lines.forEach((line, i) => {
    if (skip.has(i + 1)) return;
    if (line.includes('"') || line.includes("'")) return; // avoid quoted-string false positives
    const hashIdx = line.indexOf("#");
    if (hashIdx <= 0) return; // full-line comment or none
    const before = line.slice(0, hashIdx);
    if (before.trim() === "") return; // comment is the only content on the line
    const spaceCount = before.length - before.trimEnd().length;
    if (spaceCount < 2) {
      findings.push({
        file,
        line: i + 1,
        col: hashIdx + 1,
        severity: "warning",
        rule: "comments",
        message: "inline comments need at least 2 spaces before '#'",
      });
    }
  });
  return findings;
}

export function checkSecretShapes(file: string, lines: string[]): Finding[] {
  const findings: Finding[] = [];
  lines.forEach((line, i) => {
    for (const { name, re } of SECRET_PATTERNS) {
      const match = line.match(re);
      if (match) {
        findings.push({
          file,
          line: i + 1,
          col: (match.index ?? 0) + 1,
          severity: "error",
          rule: "secret-shape",
          message: `line looks like it contains a ${name} — defense-in-depth check, gitleaks is the source of truth`,
        });
      }
    }
  });
  return findings;
}

export function checkSyntax(file: string, docs: Document[], lineCounter: LineCounter): Finding[] {
  const findings: Finding[] = [];
  for (const doc of docs) {
    for (const err of doc.errors) {
      const pos = err.pos?.[0] ?? 0;
      const { line, col } = lineCounter.linePos(pos);
      findings.push({ file, line, col, severity: "error", rule: "syntax", message: err.message });
    }
  }
  return findings;
}

export function lintText(file: string, text: string): Finding[] {
  const lines = text.split("\n");
  const lineCounter = new LineCounter();
  const docs = parseAllDocuments(text, { lineCounter });
  const skip = blockScalarLines(docs, lineCounter);

  return [
    ...checkSyntax(file, docs, lineCounter),
    ...checkTabs(file, lines, skip),
    ...checkIndentSpacing(file, lines, skip),
    ...checkTruthy(file, lines, skip),
    ...checkCommentSpacing(file, lines, skip),
    ...checkSecretShapes(file, lines),
  ];
}
