# CLI-free Shipper consumer

From the repository root, with Rust 1.95 and the fixture's locked dependencies
cached, run:

```sh
sh fixtures/embedding/check.sh
```

To fill an empty Cargo cache, first run
`cargo fetch --locked --manifest-path fixtures/embedding/Cargo.toml`. The
proof itself runs Cargo offline and the executable's only HTTP requests go
to its own loopback server.

[Cargo.toml](Cargo.toml) declares the fixture's normal dependencies in a
standalone Cargo workspace. It points at the checkout's `shipper` facade
with default features disabled. [Cargo.lock](Cargo.lock) pins the fixture's
resolution. To inspect the normal edges checked by the script, run:

```sh
cargo tree --offline --locked --manifest-path fixtures/embedding/Cargo.toml --edges normal --target all
```

[The executable](src/main.rs) creates two local packages whose dependency
order differs from their alphabetical order. It builds the same plan twice
and checks the plan ID and package order. For an unknown package, it prints
the planning error and explains how to correct the selection before retrying.

The example supplies a custom `Reporter` to a session-isolated preflight.
Verification is disabled to avoid invoking any `cargo publish` command,
including dry runs. The local registry responds only to the expected read
requests, and ownership checks are disabled. The sample reads the persisted
audit events through `EventLog` and round-trips the terminal `PublishEvent`.
It interprets `NotProven` as incomplete readiness evidence because ownership
was not checked. The scratch workspace and its audit log are cleaned up on
exit. No release credentials are required.

The fixture uses the facade's supported planning, engine, configuration,
state and typed-evidence APIs. The doc-hidden `shipper_core::cli_bridge` and
engine internals are outside this embedding contract. The public API is
pre-1.0. Consult the project's migration guidance before upgrading. This
fixture verifies a source checkout. Its lockfile and local path dependency
describe this proof's resolution rather than a published release.
