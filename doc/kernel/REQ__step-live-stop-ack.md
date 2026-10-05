# Stepped live and local stop acknowledgement

StepGameplay owns the shared remote output queue for solo and StepLocalGameplay.
Extend the actual Stop ACK evidence used by stepped replay to this shared control
owner. A Runtime queue admission, report or take_commands batch is not proof that
the remote Worklet queue accepted a Stop. Start at zero and expose the aggregate
acknowledged_stop_commands() on both solo and local owners, readable after failure.
This is the shared output queue's count, not a fabricated per-member allocation.

Factor acknowledge_batch_with_stop_evidence beside the existing authoritative
acknowledge_batch helper and use it in StepGameplay and StepReplay. Stage only the
small checked OwnedStopEvidence copy from the exact retained admitted prefix;
commit after the existing validator accepts full success or AudioRejected's valid
partial prefix. Invalid sequence/count/incomplete success or repeated/unsolicited
ACK never earns evidence. Preserve original typed batch errors and owner fences,
including partial failure after Stops were accepted. No retries, command-vector
clones, new wire formats, error/report shape changes or duplicate ACK algorithms.
An evidence count overflow is an existing typed AudioCountOverflow; StepReplay
uses its existing Acknowledgement wrapper. Keep normal finite acknowledged command
total semantics unchanged; a partial failed ACK never claims output completion.

The live shared owner uses existing explicit owned section/cursor validators with
its actual ACK ledger for finite and unlimited output. Keep generic validators
strict and all original clock/grid/counter/chronology/endpoint checks, including
raw counters. unknown_stops must fit actual ACK count and commands_applied; all
other diagnostic semantics remain unchanged. Validate before adopting output
frontiers or before a browser adapter can use a report to feed BGM. A failed local
member's Stop ACK may explain the shared queue diagnostic while healthy members
and BGM continue. The numeric member fence and original committed prefix remain.

Preserve existing finite/unlimited completion criteria and drain resets in this
slice. Stop evidence is not a completed gameplay/clear/fail decision, physical
silence or native presentation. A numeric-fenced member still needs explicit
terminal-readiness integration; do not finish from ACK alone or fabricate judge
results for its unplayed notes. Native output ownership and the high-level mine
admission guard remain separate unfinished work.

Independent deferred fixtures cover actual solo/local fatal runtime operations,
queued Stop prefixes and remote queues/Mixer; full/partial/invalid/repeated ACK,
original error evidence and readable acknowledged counts, prepared versus remote
admission, shared survivor/BGM output with inactive Stops, strict generic and
forged/excess/other-error output rejection without frontier adoption, finite and
normal no-mine regression. Keep existing stepped replay fixtures unchanged.
Assertions, real Worklet/browser/device execution, performance and formal QA/
review remain deferred; authorized compile-only checks are not acceptance proof.
