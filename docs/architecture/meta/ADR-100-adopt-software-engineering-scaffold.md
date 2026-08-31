---
status: Accepted
date: 2026-08-31
deciders:
  - aaronsb
related:
  - ADR-200
  - ADR-300
  - ADR-400
---

# ADR-100: Adopt software engineering scaffold

## Context

dotbanner began as a bash prototype in the operator's private dotfiles store: a
`magick` + `chafa` pipeline that renders terminal banners from real TTF/OTF
outlines and bakes the results into toilet `.tlf`, figlet `.flf`, and cfonts
JSON fonts. A prior-art survey found no existing TLF generator of any kind and
a live audience for colored Unicode figfonts (PhMajerus/FIGfonts, hand-drawn),
so the project graduates to a public repository under the dotarchy org and a
Rust rewrite.

This is the founding scaffold decision for that repository, made at greenfield.

## Decision

- **Repository**: `dotarchy/dotbanner`, public, MIT licensed.
- **Development nature**: principally AI-developed — the operator directs,
  Claude implements. CODEOWNERS is annotated with agent roles per path.
- **ADRs**: domain-numbered via the vendored `docs/scripts/adr` tool, five
  domains matched to the pipeline architecture: meta (100s), recipe (200s),
  engine (300s), symbolizer (400s), surfaces (500s).
- **Workspace**: cargo workspace with `crates/dotbanner-core` (recipe, engine,
  symbolizer, format emitters) and `crates/dotbanner` (CLI + TUI binary).
- **Delivery**: PR-always, regular merge preferred, scaffold and subsequent
  work land through branches.
- **Ways**: two project-local ways at birth — `recipe` (schema stability) and
  `symbolizer` (determinism invariants).
- **Docs**: README (gist-first), CONTRIBUTING, tech-debt register seeded with
  bash-prototype learnings, and a maintained system context diagram
  (`docs/architecture/context.mmd`).

## Consequences

### Positive

- Decisions carry provenance from day one; the bash prototype's lessons land
  in ADRs and the tech-debt register instead of session memory.
- The public repo gives the recipe-sharing culture a home.

### Negative

- Scaffold overhead for a project that is currently one contributor.

### Neutral

- An `rfc` domain was considered and declined for now; community-facing
  proposals can adopt one later without renumbering (600s are free).
- The bash prototype remains deployed from the dotfiles store until the binary
  supersedes it.

## Alternatives Considered

- **Stay a bash script in the dotfiles store** — rejected: four runtime
  dependencies, no TUI, and the prior-art finding argues for a public tool.
- **Personal repo (`aaronsb/dotbanner`)** — rejected: dotarchy is the org
  identity for this family of tools.
- **RFC domain at birth** — deferred: no community yet.
