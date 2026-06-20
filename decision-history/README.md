# Decisions

This directory holds **Architecture Decision Records (ADRs)** for Turnstile — short
documents that each capture one significant decision: the context that forced it,
the choice made, the alternatives weighed, and the consequences accepted.

An ADR is a point-in-time record, not living documentation. Once accepted, an ADR
is not rewritten when the world changes; instead a new ADR supersedes it and the
old one is marked `Superseded`. The trail of records is the value — it tells a
future reader _why_ the system is the way it is, including the roads not taken.

For the canonical description of the practice, see Michael Nygard's original
article, [Documenting Architecture Decisions](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions),
and the community hub at [adr.github.io](https://adr.github.io/).

## Conventions

- One decision per file, named `NNNN-short-title.md` with a zero-padded sequence
  number (`0001-...`, `0002-...`). Numbers are never reused.
- Status is one of `Proposed`, `Accepted`, `Deprecated`, or `Superseded`.
- When a decision replaces an earlier one, set the old ADR's status to
  `Superseded` and link the two.
- Keep them short. An ADR that needs many pages is usually several decisions.

## Index

| ADR                                                                 | Title                                                                   | Status   |
| ------------------------------------------------------------------- | ----------------------------------------------------------------------- | -------- |
| [0001](0001-proof-assistant-configuration-and-failure-surfacing.md) | Proof Assistant configuration and failure surfacing                     | Accepted |
| [0002](0002-cache-linux-ci-dependencies.md)                         | Cache Linux CI system dependencies via a shared composite action        | Accepted |
| [0003](0003-backend-source-of-truth-for-formal-proof.md)            | Backend as single source of truth for formal-proof text and annotations | Accepted |
| [0004](0004-goal-state-cursor-anchored-cache-backed-cards.md)       | Goal state as cursor-anchored, cache-backed after-state cards           | Proposed |
