# Rails

Rails is this repository's durable source-of-truth framework.

It separates:

- proposals: why work exists
- specs: what behavior must be true
- ADRs: durable architecture decisions
- lanes: focused implementation trackers
- support maps: what users may believe and what proves it
- policy references: governed ledgers and enforcement sources
- closeouts: what landed, what proved it, and what remains

## Scope

Rails owns `.rails/` and the human-facing docs that explain it.

Rails does not own, modify, migrate, or validate:

- `.codex/`
- `.spec/`
- `.claude/`
- `.jules/`

Those may overlap conceptually, but they are external tool or agent spaces.

## Source-of-truth rule

Proposal says why.
Spec says what.
ADR says what decision.
Lane says what sequence.
Support says what users may believe.
Policy says what exceptions exist.
Receipts say what proved it.
Closeout says what happened.
