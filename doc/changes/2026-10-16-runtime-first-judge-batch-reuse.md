# Runtime 첫 owned 판정 묶음 재사용 — 2026-10-16

Runtime이 Judge의 owned 판정 Vec를 빈 report에 다시 누적하던 지점에서 첫 묶음의 소유권을 넘긴다. 이미 판정이 있으면 기존 extend를 유지한다. 키음의 `&events` 조회, hazard 수집, bound 입력 추가, 첫 오류의 break와 최종 발행 순서는 바꾸지 않았다. public API·필드·port·native/UI adapter·의존성은 추가하지 않았다.

TASK__runtime-first-judge-batch-reuse / run01a1413b-aeb8-728c-8465-9c9dfa90cc79, base647fd4b51c97b8343a5a903d1adfd3fb93ccd66e. 아래는 실제 root Verify 결과이며, 정식 독립 code review→qa-cli 및 owning 종료 검사는 아직 남았다. 전체 player Goal은 미완료다.

## 동작 회귀

기존 `crates/beatkernel/tests/runtime_input_disposition.rs`의8개를 보존·보강하고3개를 추가했다. 양 API를 각각 독립된 전체 RuntimeReport와 실제 queue FIFO에 대조한다.

- `mixed_empty_and_hit_batches_preserve_literal_reports_and_sound_fifo`: bindings[3,2,4,1,5], 빈→hit2→빈→hit1→빈 결과를 보존한다. 판정 키음2→1 뒤 fallback3→4→5를 발행하며 hit1/2의 다른 fallback은 억제한다. 원본 정규화 metadata·mapping quality·시간·상태를 확인한다.
- `mixed_partial_error_preserves_complete_literal_prefix_and_stops_fanout`: [3,1,4,2,5]에서 Refuse2 이전의 빈/hit1/빈 결과와 소리를 보존한다. 양 API의 전체 결과·정확한 오류·queue empty tail 및 object1 Completed/object2 Pending/control5 fresh를 검사한다. 관측 API의 dispositions/refused도 별도 검사한다.
- `dense_passive_first_batches_preserve_results_and_report_observed_capacity`: builtin time0의 zero-window 노트가 time100에 만료되는 모든 결과·순서·입력 부재·상태·queue를 독립 기대값으로 검증한 뒤 capacity를 관측한다.

## 실제 요청량 변화

진짜 before는 이전 최적화 이후 독립 QA의 `target/wf/judge-refresh-qa-cli-14125/meter-{debug,release}.log`다. 그 당시 meter SHA256은 `9f99bcef5e22021180a5702ef2dd7a4c0304bf1fd443d9b0c6942012929fb305`. 새 source를 고정하고 그 meter를 바꾸지 않은 실제 post를 먼저 실행했다. 모든66개 이름·params·target/type metadata·전체 Requests를 비교했다. 교정9/공개 API57과14개 테스트가 양 profile에서 실행됐다.

양 profile 모두 **26개 감소 /40개 동일 /증가0**이다. 감소한 각 사례는 alloc와 dealloc 각각 attempted/succeeded1회 및 requested_bytes544B가 줄었다. zeroed/realloc/old-realloc bytes/overflow는 변하지 않았다. 직접 Judge·교정·거절·no-result 대조도 그대로다. 아래 count/B는 각각 allocator 요청 횟수와 요청 byte 합이며 live memory가 아니다.

| 사례 | alloc before → after | dealloc before → after |
|---|---|---|
| `judge.legacy.hit.fresh` | 5/1004 → 5/1004 | 3/344 → 3/344 |
| `runtime.legacy.hit.fresh` | 9/2368 → 8/1824 | 4/888 → 3/344 |
| `runtime.observed.hit.fresh` | 10/2624 → 9/2080 | 4/888 → 3/344 |
| `runtime.association.reused` | 7/2296 → 6/1752 | 4/888 → 3/344 |
| `runtime.prefix.4096` | 7/2296 → 6/1752 | 4/888 → 3/344 |
| `runtime.observed.fanout` | 14/3512 → 13/2968 | 8/1776 → 7/1232 |
| `runtime.mapper.allocating` | 12/2686 → 11/2142 | 6/950 → 5/406 |
| `solo.audio.admitted` | 10/2720 → 9/2176 | 5/1240 → 4/696 |
| `group.64.last` | 10/2720 → 9/2176 | 4/888 → 3/344 |

전체26개 감소 이름은 다음과 같다. 다른40개도 원본 비교 JSON에 모두 보존한다.

`group.1.automatic`, `group.1.first`, `group.1.last`, `group.2.first`, `group.2.last`, `group.4.first`, `group.4.last`, `group.64.first`, `group.64.last`, `group.failure.disconnected`, `group.failure.full`, `runtime.association.reused`, `runtime.audio.admitted`, `runtime.audio.disconnected`, `runtime.audio.full`, `runtime.external.accepted`, `runtime.legacy.hit.fresh`, `runtime.mapper.allocating`, `runtime.mapper.same-domain-control`, `runtime.observed.fanout`, `runtime.observed.hit.fresh`, `runtime.prefix.0`, `runtime.prefix.4096`, `solo.audio.admitted`, `solo.audio.disconnected`, `solo.audio.full`.

실측 후11개 match arm의26개 literal ceiling만 좁혔다. 나머지40개,14개 테스트·66 workload·오라클·allocator wrapper·target 조건·unknown case/13개 scalar/overflow 거부는 유지한다. 예산을 검사 중인 결과에서 생성하지 않는다. 최종 양 profile도14/0·각66상한 PASS이며 첫 post와 모든 Requests가 같다.

## 반환 버퍼 용량

`RUNTIME_BATCH_CAPACITY` 원본은 양 API×[1,4,5,17,257]별 정확히1개, debug/release 각각10개·총20개다. 모든 결과 검사가 먼저 통과하며 아래는 양 API/profile에서 같았다. 숫자는 공개 capacity 보장이 아닌 관측값이다.

| len | capacity | JudgeEvent bytes | len bytes | capacity bytes | spare bytes |
|---:|---:|---:|---:|---:|---:|
| 1 | 4 | 136 | 136 | 544 | 408 |
| 4 | 4 | 136 | 544 | 544 | 0 |
| 5 | 8 | 136 | 680 | 1088 | 408 |
| 17 | 32 | 136 | 2312 | 4352 | 2040 |
| 257 | 512 | 136 | 34952 | 69632 | 34680 |

이는 논리적 element-storage byte이며 누적 allocator 요청량·live/peak memory·RSS가 아니다. 이전 반환 capacity는 측정하지 않았다. 따라서 retained-memory의 전후 감소/증가 비교는 성립하지 않는다. 첫 묶음의 여유 capacity도 넘겨받는 tradeoff를 보존하며 숨기기 위한 shrink/copy/pool은 추가하지 않는다.

## 실제 실행과 원본

cached rustc1.98.1(48a229cea)/LLVM22.1.8, Linux x86_64, pointer8/PhysicalInputEvent144/JudgeEvent136/RuntimeReport344B. `target/toolchain/env.sh`, RUSTUP_AUTO_INSTALL=0/CARGO_NET_OFFLINE=true/CARGO_BUILD_JOBS=2, 기존 host CC/AR, foreign RUSTFLAGS 해제. 설치·다운로드·장치 생성·환경 복구·push는 하지 않았다. 정확한 argv/env/compiler/source·산출물 전후 hash는 각 stage의 invocations.json에 있다.

| 명령 | 실제 root 결과 |
|---|---|
| `cargo test -p beatkernel --locked --offline -j2 --no-fail-fast -- --nocapture` | 840/0, ignored0,84 summaries |
| `cargo test -p beatkernel --test runtime_input_disposition --release --locked --offline -j2 -- --nocapture --test-threads=1` | 11/0 |
| `cargo test -p beatkernel-bms-runtime --no-default-features --lib local_runtime --locked --offline -j2` | 36/0,2870 filtered |
| unchanged/final `cargo test -p beatkernel-bms-runtime --no-default-features --test runtime_input_allocations --locked --offline -j2 [--release] -- --nocapture --test-threads=1` | 각 profile·각 stage14/0,66 raw/66 checked |
| `cargo check -p beatkernel --target wasm32-unknown-unknown --locked --offline -j2` | exit0,typing |
| scoped `rustfmt --check --edition 2021 --config skip_children=true` | 변경 Rust3개 exit0 |

증거 디렉터리: `target/wf/runtime-first-batch-ac002-14126/`. core/release/local_runtime은 correctness/, 원래 meter의 실제 post는 before-budgets/, 좁힌 상한·WASM은 final-budgets/에 있다. `post-batch-measurements.json` 두 파일은 각66개 전체 Requests와26개 delta를 포함하고 correctness/capacity-observations.json은20개 행과 산술/완전성 확인을 기록한다. 원본은 ignored 로컬 산출물이며 커밋된 소스/테스트와 동일하게 원격 배포된 자료가 아니다.

| 원본 로그 | SHA256 |
|---|---|
| correctness/core-debug.log | `48a4210375a1a579a6eb443c492efe295775817762072732d740d9ccee465d4a` |
| correctness/core-routes-release.log | `a32b50829fe61122f1f2822bc779271beaf961eae3a0e6d94eb0b2327aacd2e5` |
| correctness/app-local-runtime.log | `7a9073864807ed15a6b6757ba402417dd100b439a8db8009c725104555370ce3` |
| before-budgets/meter-debug.log | `1eb271f27048416e6528df835012a09cd2f54a45b6177703c6ce6122618bc638` |
| before-budgets/meter-release.log | `baa1d9c5df9994df2f4b1fd27c3dc05d397b91809efb9a50cbd4a72e96d646b4` |
| final-budgets/meter-debug.log | `683a0663b961ab232de41769cbdf0779029473f3a6c07548f05923cbff6b2f2d` |
| final-budgets/meter-release.log | `52ea4c76d92cf3d4369f95f67e1f9a22f9186cfaaa6efb27118d900f420d8d7c` |
| final-budgets/core-wasm.log | `1c156f76137954deeadd0bb3df2a21c0caa8915d305a628e915f8991b8066282` |

측정 source identity:

- `crates/beatkernel/src/runtime/mod.rs`: `13f8b9fd4fb6c6ff3d1f37e1b79d0dcd6bfad137edc20e639ef9be520b31f66e`, 47120B.

- `crates/beatkernel/tests/runtime_input_disposition.rs`: `50086b44c51246abd7bf3fdb9b76e4f3c12734b796a5dce75908e7381c2ffd07`, 42676B.

- `app/tests/runtime_input_allocations.rs`: `02925723960dc3ccd1d5ef2ab1d2a8394bbe617fdfabdf7616cc9a44ce51d4a0`, 87462B.

## 산출물과 남은 범위

이번 Cargo 후 현재 산출물은 다음과 같다. native before675d5cbd…/181917864B에서 아래 새 hash로 바뀌었다. 과거 c43 GUI 결과를 이 바이너리에 이월하지 않는다. 6f/675는 이전 build identity다. browser4개는 재빌드하지 않았으므로 source 최적화가 served WASM에 포함됐다고 주장하지 않는다.

| 산출물 | bytes | SHA256 |
|---|---:|---|
| `app/web/pkg/beatkernel_bms_runtime.js` | 232884 | `33733edc7cb07d63a7f79fb9c813706cf811b193230ef561a8c4c826f90fb89e` |
| `app/web/pkg/beatkernel_bms_runtime_bg.wasm` | 5460000 | `d2bdf298cd3aec4ee87b33e1e48fb389881f33468bee96062f52e23961a7891c` |
| `app/web/audio-pkg/beatkernel_bms_runtime.js` | 19382 | `eaa1340083a7e6dc40b6cecbd8b9c888b8f07ac311ddf0d9d26116c0f849eb40` |
| `app/web/audio-pkg/beatkernel_bms_runtime_bg.wasm` | 175806 | `52940c9236a5b531b819c58d111379e66ecf09c73494dfa73147b13fedd508a0` |
| `target/debug/beatkernel-bms-runtime` | 181918240 | `e91676219fd5078b8c50a6c063a622aaac1c304ac66d6fee654498a3c4cf59fe` |

## Known ceiling

Known ceiling: allocator 요청량 감소는 CPU/경과 지연·live memory·세계 최고 성능·Native clock/lock/IO·임의 callback·다른 thread·물리 장치·현재 GUI/browser 재생의 개선 증거가 아니다. Group64의 일정한 할당 요청 수는 CPU routing이 상수 시간임을 뜻하지 않는다.

BK-WBS-13.06.01은 넓은 lock/IO 및 native/device 관측이 남아 W다. 전체136/489(27.81%), 오디오12/27, 원래91/193 및 전체 Goal 미완료를 유지한다. 새 public API/직렬화/아키텍처/라이선스 결정이나 요구 축소는 없다. 정식 독립 review→QA 및 owning verification/close는 후속 gate다.
