# Portable room network values and port

Room options, commands, reply diagnostics, rosters, snapshots and joined outcomes
move into `room_network_model`, alongside the injectable `RoomNetworkPort`.
The native worker implements that port and preserves its old public model names
as aliases. The room controller imports its values from the portable contract.
Option validation remains shared before acquisition; original IDs, signed room
time, receipt fields and retained Arc ownership keep their existing semantics.

## Known ceiling

The concrete controller defaults, native waiting adapters, result construction,
cleanup diagnostics and worker thread/stream/clock ownership remain native.
Creating a snapshot or terminal outcome is not proof of Commit, final ACK or
successful cleanup. This extraction adds no measured performance evidence.

Five independent pure model/port fixture groups are prepared for later execution.
After both writers stopped, scoped formatting completed and all four compile-only
checks exited zero: workspace/all-targets with WebTransport, no-default-features
WebTransport/all-targets, WASM browser/library and WASM browser-audio/library.
The first workspace check exposed a missing worker `GroupPrefix` import; the
owning implementation lane restored it before the successful repeat. Remaining
warnings concern existing unused code. WASM library checks compile the portable
module, but do not compile its test child. Tests, runtime/platform acceptance,
formal review and QA remain deferred.
