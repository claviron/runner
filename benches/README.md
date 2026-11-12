# Runner benchmarks

## Supervised Link transports

These targets measure Runner applications directly, through the production Link
backend or upstream client. Both sides boot Claviron with their own supervisor
and fleet. Client workers generate requests from a managed Runnable; they open
no frontend HTTP listener. Application workers use the same benchmark router. The comparison includes each
backend's target selection, admission and pool model; it does not isolate raw
socket or shared-memory copying cost.

```sh
cargo bench --features bench \
  --bench link-concurrent -- \
  --workload plain --client-workers 2 --application-workers 2 \
  --edge-threads 1 --application-threads 1 \
  --in-flight 4 --pace 1 \
  --warmup-seconds 2 --measurement-seconds 10 --samples 5 \
  --queue-timeout 100ms --output .benchmarks/link/orbit.json
```

`h2-concurrent` defaults to TCP/H2c; `uds-h2-concurrent` defaults to UDS/H2.
Every executable also accepts `--transport orbit|h2c|uds-h2`, so one binary can
compare all three. The harness boots a client and a Runner without Edge. Keep
optional features such as `metrics` identical between comparisons.

### Load and acceptance

Every run starts at **one outstanding request per client worker**. Each accepted
phase adds `--pace` until `--in-flight`, the **per-worker ceiling**, or the first
rejected phase. With two workers and `--in-flight 4`, total offered concurrency
rises through 2, 4, 6 and 8. This replaces the old fixed-total-concurrency harness.
A slot sends its next request after its previous request finishes; no arrival
rate or receiver occupancy is guaranteed.

[config/link/client.yaml](config/link/client.yaml) declares a whole-response
`request_timeout: 100ms` and `accept: 99.99`. A success requires a 2xx status and
the exact complete body within that deadline. Preflight must pass for every
client worker. Each phase has its own untimed warmup and measured windows;
acceptance uses outcomes completed within those windows. Requests crossing a
window boundary count where they complete. Drain completes before the next
phase. No latency percentiles are measured.

Every report retains success, 503, 502, other statuses, transport errors, invalid
bodies and deadline failures, including a rejected phase. `--keep-errors` lets a
rejected measurement exit successfully; it does not continue increasing load.

`--edge-threads` names the client process's main Tokio thread budget;
`--application-threads` names the application's. Dedicated executors are selected
separately in the client and Runner fixtures. These are per-process settings,
not CPU affinity or a total host CPU allocation. Each family has an additional
supervisor master, excluded from its worker CPU total.

### Capacity controls

[config/link](config/link) is the default fixture directory; `--config` selects
another. Each run writes its resolved YAML and launch data beneath
`.benchmarks/link/run-…/`, and embeds the YAML in its JSON report.

- Orbit: `--max-in-flight 128` bounds application admission through body/session
  completion. Omit it for the unbounded admission control. SHM lane/inbox geometry
  remains independently bounded; unbounded admission does not mean infinite slots.
- H2: the common upstream fixture sets `max_live: 1`, `max_idle: 1` and
  `max_streams: 128`. `--max-live` and `--max-streams` override the same pool
  settings for TCP and Unix origins. There is no H2 `min_idle`.
- `--queue-timeout 100ms` declares the production admission/acquire waiting budget.
  For Link this includes receiver and creator-slot waiting under one budget.
  It is separate from the whole-response client deadline. To distinguish budget
  expiry from a client deadline, select an appropriately larger client deadline
  in the fixture before the comparison.
- `--workload echo --body-bytes N` validates a complete echoed request body.
  `--buffer-size` selects Link request buffering policy; keep buffered/streamed
  and plain-body measurements separate.

For an admission comparison, use one binary, identical geometry and the same
ramp with omitted `--max-in-flight` versus `--max-in-flight 128`. A bound equal
to offered concurrency measures admission overhead; a smaller bound also measures
waiting and pacing. Preserve rejected phases instead of comparing successful
responses to a stream of immediate 503s.

Worker counts must fit `CLAVIRON_FLEET_CAPACITY` independently for each family,
including its master. Orbit's selected fleet geometry must fit both families
combined, including both masters. For 40 + 40 workers, compile with
`CLAVIRON_FLEET_CAPACITY=128` and `ORBIT_LINK_FLEET_CAPACITY=128`, and select
compatible geometry. These are **build-time** ceilings, not runtime flags.

### Reports and diagnostics

Schema 3 records all worker/master identities, requested concurrency, resolved
fixtures, client policy, raw per-process elapsed windows, outcomes and user/system
CPU. Client and serving application CPU are reported per success and per attempt;
masters are reported separately. Client CPU includes load generation and response
validation. Windows are requested concurrently but start/end independently, so
application CPU per client success is an overlapping-window estimate.

`--diagnostics` separately records client request-future polls, pending polls,
waker calls, completion time and returned transport errors. Its clocks, wrapped
wakers and counters perturb execution. Never compare diagnostic CPU/throughput
with clean runs. Waker calls are not IPC doorbells, kernel wakes, application polls
or admission waits. The completion high-water mark includes warmup and prior
phases. macOS context-switch counters are marked unavailable.

Keep repository revisions, dirty source hashes, binary hash, OS/architecture and
run order alongside raw JSON. Old two-process fixed-load reports are historical
controls, not identical measurement boundaries. `results/` retains such records;
new local output defaults to the ignored `.benchmarks/` area.

The orchestrator stops client generation first, asks each production master to
drain its workers, then clears only its own isolated fleets after both families
exit and an exclusive fleet lock succeeds. Retained SHM under a generated name
causes startup to fail rather than being adopted. Ctrl-C follows the same owned
shutdown path; no private kill deadline is introduced.
