# Benchmark architecture

### Supervised Link transports

Each entry point uses [harness](harness/mod.rs). The orchestrator starts two
Claviron masters; each production supervisor owns its worker processes. Client
workers run a load-generating `Runnable` without an HTTP frontend. Application
workers run the same Runner and benchmark router for every transport.

| File | Cargo target | Client → application |
| --- | --- | --- |
| [link/concurrent.rs](transport/link/concurrent.rs) | `link-concurrent` | Production `LinkBackend` → `OrbitDoor`. |
| [h2/concurrent.rs](transport/h2/concurrent.rs) | `h2-concurrent` | Production upstream H2 pool → TCP runner listener. |
| [h2/unix.rs](transport/h2/unix.rs) | `uds-h2-concurrent` | The same H2 pool and listener driver over a shared Unix socket. |

[config/link](config/link) declares the client policy, Runner execution,
transport, upstream capacity, Orbit geometry and supervisor policy. Resolved
fixtures and reports live under ignored `.benchmarks/link/`. The minimal build
has no Edge dependency. Load starts at one outstanding request per client worker
and rises after each accepted phase; `--in-flight` is the per-worker ceiling.

| Harness module | Responsibility |
| --- | --- |
| [config.rs](harness/config.rs) | CLI, client policy and per-process launch data. |
| [fixtures.rs](harness/fixtures.rs) | Resolve isolated configuration and validate compiled capacities. |
| [boot.rs](harness/boot.rs) | Claviron master/worker re-entry; Server client and Runner application. |
| [service.rs](harness/service.rs) | Managed control Runnable, preflight, load generation and drain. |
| [backend.rs](harness/backend.rs) | Real Link and upstream dispatch; complete response validation. |
| [process.rs](harness/process.rs) | Exact child identities, readiness, control and supervisor shutdown. |
| [wire.rs](harness/wire.rs) | Typed control messages, outcomes and process CPU windows. |
| [report.rs](harness/report.rs) | Phase ramp, raw worker windows, role totals and owned fleet cleanup. |
| [diagnostics.rs](harness/diagnostics.rs) | Opt-in client-future polls/wakes and returned error observations. |

CPU is sampled in every worker and master. Client, serving application and
supervisor totals remain separate. Each process reports its own elapsed window;
control fan-out is concurrent, not a globally atomic clock boundary. Outstanding
counts describe offered client work, not measured receiver occupancy. Clean CPU
runs and diagnostic runs are separate. See [usage and limits](README.md#supervised-link-transports).
