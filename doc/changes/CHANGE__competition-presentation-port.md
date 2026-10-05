# Competition presentation policy

Competition display payloads previously belonged to the player UI module, and
native competition owners built UI snapshots while reading Instant directly.
The payloads and lazy solo/group projection now belong to a pure business module
with an explicit CompetitionPresentationHost for attachment, monotonic display
time and publication. Original player::* names remain re-exports. Native
competition observation/publication delegates to the actual pure policy, with
explicit host-injected entry points; its bridge owns UI lookup/effects and the
display clock. No new crate or dependency was introduced.

The 50ms interval starts after successful publication, preserving the old slow
publication behavior. Suppression precedes projection. Refusal leaves cadence
unchanged, and post-effect clock failure/regression reports the partial success
without rolling back the already performed effect. Existing remote-roster/order
validation, unequal roster mapping, retained disconnected prefixes and sanitized
64-scalar basenames remain intact; remote scores never enter local judgment.

Eight independent deferred fixture groups cover cadence, forced/suppressed
updates, refusal/retry, memory replay ghosts, full remote validation, cohort
ordering/cardinality, slow effects and post-effect clock faults. Actual allocation
instrumentation and native network disconnect behavior are not tested here.
After both writers stopped, scoped rustfmt and git diff --check completed.
All four root compile-only checks exited zero: workspace/all-targets with
webtransport; runtime/all-targets without defaults with webtransport; WASM
browser library; WASM browser-audio library. Existing dead-code and ClockPair
import warnings remain. No assertions, formal review/QA, real drivers or
comparative benchmarks have been executed; compilation is not test success.

## Known ceiling

경쟁 소유자의 네트워크·저장소·설정 대기·터미널 출력은 아직 네이티브 구현에 연결됨 — 후속 경계 분리에서 처리.

Display injection does not make the remaining native owners pure or establish
full layer separation, SQLite-grade reliability or full player completion.
