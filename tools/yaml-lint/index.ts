#!/usr/bin/env bun

/**
 * yaml-lint — Bun/TypeScript YAML gate. Runs from lornu-ai/ci-checks as a
 * composite action (.github/actions/yaml-lint) — consumer repos don't vendor
 * this file, they just `uses:` it. Core checks live in lib.ts (unit tested
 * via lib.test.ts); this file is just glob scanning + CLI wiring.
 *
 * Not a yamllint reimplementation: ports the rules that actually fire in
 * infra-code (see its docs/proposals/slim-pr-ci-bun-yaml-kustomize-gitleaks.md
 * §5), mirroring `.yamllint.yaml`'s tuning (2-space indent, truthy values,
 * disabled line-length/document-start/key-duplicates/empty-lines) without
 * pulling a Python runtime into CI. Does NOT implement yamllint's full
 * `indentation.indent-sequences: consistent` state machine — deliberately;
 * a correct implementation needs full AST awareness of flow vs. block
 * collections, and the simpler "indentation is a multiple of 2" check below
 * already catches the dominant real failure class here with near-zero
 * false-positive risk. Full parity is future work, not silently assumed.
 *
 * Scans the consumer repo's checkout (process.cwd() — the caller's working
 * directory, not this action's own checkout path) for *.yaml/*.yml under
 * `YAML_LINT_ROOTS` (comma-separated; defaults below), skipping roots that
 * don't exist, honoring the same ignore globs as .yamllint.yaml.
 *
 * `YAML_LINT_ENFORCE=false` runs every check and prints every finding but
 * always exits 0 — warn-mode for initial adoption on a repo with existing
 * debt, same shape as fft-rust-gate.yml's `enforce` input.
 */

import { Glob } from "bun";
import { lintText, type Finding } from "./lib.ts";

const DEFAULT_ROOTS = ["crossplane", "flux", "clusters", "k8s", ".github/workflows"];
const ROOTS = process.env.YAML_LINT_ROOTS
  ? process.env.YAML_LINT_ROOTS.split(",").map((r) => r.trim()).filter(Boolean)
  : DEFAULT_ROOTS;
const ENFORCE = process.env.YAML_LINT_ENFORCE !== "false";

// Mirrors .yamllint.yaml's `ignore:` block.
const IGNORE_GLOBS = [
  "**/upstream/**",
  "**/cert-manager-*.yaml",
  "**/external-secrets-*.yaml",
  "**/crossplane-1.*.yaml",
  "**/kube-prometheus-stack-*.yaml",
];

function isIgnored(relPath: string): boolean {
  return IGNORE_GLOBS.some((pattern) => new Glob(pattern).match(relPath));
}

async function collectFiles(): Promise<string[]> {
  const files: string[] = [];
  for (const root of ROOTS) {
    // dot: true — several of the default ROOTS (.github/workflows) are
    // dot-prefixed, and Bun's Glob excludes dot segments by default. Without
    // this, .github/workflows silently scans zero files.
    const glob = new Glob(`${root}/**/*.{yaml,yml}`);
    for await (const relPath of glob.scan({ cwd: process.cwd(), onlyFiles: true, dot: true })) {
      if (!isIgnored(relPath)) files.push(relPath);
    }
  }
  return [...new Set(files)].sort();
}

function annotate(f: Finding): string {
  const level = f.severity === "error" ? "error" : "warning";
  return `::${level} file=${f.file},line=${f.line},col=${f.col},title=yaml-lint/${f.rule}::${f.message}`;
}

async function main(): Promise<number> {
  const files = await collectFiles();
  if (files.length === 0) {
    console.log("yaml-lint: no YAML files found under scan roots — nothing to do");
    return 0;
  }

  const findings: Finding[] = [];
  for (const file of files) {
    const text = await Bun.file(file).text();
    findings.push(...lintText(file, text));
  }

  const errors = findings.filter((f) => f.severity === "error");
  const warnings = findings.filter((f) => f.severity === "warning");

  for (const f of [...errors, ...warnings]) {
    console.log(annotate(f));
  }

  console.log(
    `\nyaml-lint: scanned ${files.length} file(s) — ${errors.length} error(s), ${warnings.length} warning(s)` +
      (!ENFORCE && errors.length > 0 ? " (warn mode — not failing the build)" : ""),
  );

  return ENFORCE && errors.length > 0 ? 1 : 0;
}

process.exit(await main());
