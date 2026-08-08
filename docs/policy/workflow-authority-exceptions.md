# Workflow Authority Exceptions

Workflow authority is reported by:

```bash
cargo xtask check-workflow-surfaces --mode advisory
```

The default remediation is to remove or narrow the capability. An exception is valid only when a durable workflow would become incorrect without one exact capability and the remaining authority is explicitly owned, bounded, controlled, and review-dated.

The machine-readable ledger is:

```text
policy/workflow-authority-exceptions.toml
```

Validate its schema and lifecycle with:

```bash
cargo run --locked -p xtask --bin workflow-authority-exceptions
cargo test --locked -p xtask --bin workflow-authority-exceptions
```

## Exact identity

Each record binds all of the following:

```text
workflow
job
step
capability
trigger
repository
finding_repository_boundary
owner
reason
covered_by
created
review_after
```

A workflow path or capability glob is invalid. Trigger tokens use the detector's sorted, comma-separated form. The finding repository boundary records the exact detector value, while `repository` records the actual repository in which the capability is accepted.

## Lifecycle

1. Run the detector in advisory mode.
2. Remove or narrow every capability that is not required.
3. For a genuinely necessary capability, add one exact ledger record.
4. Record why removal would make the workflow incorrect and which controls bound the capability.
5. Set a near review date.
6. Keep the accepted finding visible in human and machine policy reports.
7. Delete the exception when the workflow, action, or output path no longer needs the capability.

The validator rejects:

- missing or empty required fields;
- nonexistent workflow paths;
- workflow or capability globs;
- duplicate finding identities;
- malformed, expired, or non-forward review dates;
- unsorted or duplicate trigger tokens;
- non-EffortlessMetrics repository identities;
- secret-like material in reasons or controls.

## Current exception candidate

The initial ledger contains one entry for the scheduled/manual Droid security scan's job-local `contents: write` capability. The scan may create a reviewable security-report branch and pull request. It no longer has OIDC authority, runs only on scheduled/manual triggers, uses a fixed action SHA and trusted runner labels, and its generated branch remains subject to pull-request review.

This record is not a broad approval for Droid workflows, all security workflows, or repository writes generally.

## Blocking integration

This PR establishes the exact schema, current record, expiry validation, and negative fixture matrix. The final #267 integration must additionally:

- reconcile each detector finding against exactly one record;
- reject unexcepted findings;
- reject unused records;
- reject workflow/job/step/capability/trigger/repository drift;
- expose `authorized_exception`, `expired_exception`, `unused_exception`, and drift states in JSON and Markdown reports;
- make every unexcepted or invalid state fail `blocking-allowlist` mode.

Unknown or unparsed authority shapes may not be hidden behind an ordinary accepted-capability record.
