# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## docs v0.1

- Write `README.md` — project landing page: tagline, what Moto is, how-it-works diagram, doc links
- Write `docs/architecture.md` — component map, design philosophy, data flow, motorcycle metaphor glossary
- Write `docs/getting-started.md` — prerequisites, `moto dev up` walkthrough, first garage, stopping
- Write `docs/deployment.md` — `make deploy`, what runs where, secrets, port-forward, production considerations
- Write `docs/security.md` — threat model, isolation layers, SPIFFE SVIDs, keybox encryption, network boundaries, compliance
- Write `docs/ai-proxy.md` — the problem, how it works, passthrough vs unified, security, configuration
- Write `docs/components.md` — reference table and short sections for each component
- Write `.github/workflows/wiki-publish.yml` — publish `docs/` to GitHub Wiki on push to main
