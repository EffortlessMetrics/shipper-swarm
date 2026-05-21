# Repo-native spec system implementation plan

Status: active
Owner: repo-architecture
Linked proposal: SHIPPER-PROP-0001
Linked specs: SHIPPER-SPEC-0001
Linked ADRs: SHIPPER-ADR-0001

## End state

Durable rails are installed under `.shipper-spec/` with contributor-facing guidance in `docs/`.

## Work items

### Work item: namespace-doctrine

Status: done
Linked proposal: SHIPPER-PROP-0001
Linked spec: SHIPPER-SPEC-0001
Linked ADR: SHIPPER-ADR-0001

#### Goal

Install the repo-owned namespace and clarify boundaries with tool-specific state.

#### Proof commands

```bash
git diff --check
```
