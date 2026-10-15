# 기본 노트 refresh의 임시 할당 제거 — 2026-10-16

## 변경과 보존한 동작

기준 revision은 `797123dc570a2e606e9049878cbcf0801e2efa00`이다. `crates/beatkernel/src/judge/engine.rs`의 private `refresh`에서 추가 control이 비어 있으면 지역 primary를 `std::slice::from_ref`로 빌린다. 비어 있지 않으면 기존 `vec![primary]` 초기 용량과 first-occurrence `contains` 중복 제거를 그대로 유지한다. 새 API·field·cache·crate·dependency·unsafe는 없다.

additional getter는 refresh당 기존처럼 한 번 호출한다. Pending/Active/Completed 인덱스, state/deadline 호출 위치와 횟수, inclusive deadline과 strictly-later successor 필터는 바꾸지 않았다. 공개 결과의 metadata·소유권, Runtime/Solo/Group의 FIFO·partial failure·poison 의미도 유지한다.

## 같은 워크로드의 실제 비용

먼저 기존 meter SHA `33b57d357cf710fbaf60921886b16dd4083c442e49a2cc0410e13c26e9d858d6`를 수정하지 않고 최적화된 source에서 debug/release를 실행했다. 각각14PASS/0FAIL/ignored0,66개 raw row·66개 기존 상한 검사가 통과했다. 교정9개·실제 public 호출57개와 이름·params·target/type metadata를 보존했으며 이전 교정 raw evidence와 모든 Requests 필드를 대조했다.

두 profile의 결과는 같았다. **31개 사례가 감소했고35개는 그대로이며 증가한 필드는 없다.** 30개는 alloc/dealloc 각각1회·4B 감소, `runtime.observed.fanout`은 각각2회·8B 감소했다. alloc_zeroed/realloc/realloc-old-bytes와 overflow는 모든 사례에서 그대로다. 아래 값은 성공한 요청 수/요청 바이트이며 attempt도 해당 성공 수와 같았다.

| 실제 사례 | alloc 회/B, 이전 → 이후 | dealloc 회/B, 이전 → 이후 |
| --- | ---: | ---: |
| `judge.legacy.hit.fresh` | 6 / 1,008 → 5 / 1,004 | 4 / 348 → 3 / 344 |
| `judge.association.reused` | 5 / 892 → 4 / 888 | 4 / 348 → 3 / 344 |
| `runtime.legacy.hit.fresh` | 10 / 2,372 → 9 / 2,368 | 5 / 892 → 4 / 888 |
| `runtime.observed.hit.fresh` | 11 / 2,628 → 10 / 2,624 | 5 / 892 → 4 / 888 |
| `runtime.observed.fanout` | 16 / 3,520 → 14 / 3,512 | 10 / 1,784 → 8 / 1,776 |
| `group.64.last` | 11 / 2,724 → 10 / 2,720 | 5 / 892 → 4 / 888 |
| `solo.audio.admitted` | 11 / 2,724 → 10 / 2,720 | 6 / 1,244 → 5 / 1,240 |
| `runtime.audio.full` | 11 / 2,660 → 10 / 2,656 | 5 / 892 → 4 / 888 |

그 실제 결과를 확보한 뒤31개 case를 포함하는13개 match arm의 정적 상한만 낮췄다. 나머지35개와 meter·workload·오라클·target 조건은 그대로다. root의 별도 정적 대조도66개 literal Requests 상한이 실제 양 profile의 관측값과 정확히 일치함을 확인했다. 현재 실행에서 자동으로 상한을 만들거나 올리지 않는다. 미등록 case,13개 scalar 각각의 초과와 overflow 거부 검사는 유지한다.

최종 상한 적용 후 root Verify도 debug/release 각각14/0/0·66 raw/66 상한 PASS이며 모든 Requests가 상한 수정 전의 최적화 실측과 같았다. source 상한은 Linux x86_64/64bit의 경험적 예산이다. 다른 target은 기존 의미·교정 검사를 유지하고 상한 미검증을 명시하며 compiler/type-size 우회 조건을 추가하지 않는다.

## 행동 회귀와 실행 범위

새 `crates/beatkernel/tests/judge_refresh_routes.rs`의4개 테스트는 빈 additional, primary 중복과 혼합 중복의 각 고유 route에서 별도 Pending 시작과 Active 입력을 실행한다. 무관 control·accepts_input false·완료 후 재진입 차단과 원본 event metadata도 대조한다. cold build canonicalization과 test 관측의 getter는 public operation 로그와 분리한다. 이 logger의 잠금·할당은 correctness fixture에만 있고 allocation meter에는 없다.

정상 Pending deadline100→200에서101의 첫 callback·200의 비만료·201의 두 번째 callback/완료를 검사한다. 같은100/뒤로 간99의 잘못된 successor는 bounded callback으로 유한 처리를 확인한다. Active interaction의 이후 정상 advance callback도 유지한다.

root 실제 실행 결과:

| 검사 | 결과 |
| --- | --- |
| 전체 core debug, 새4개와 기존 replay/generalization/expiry 및 rustdoc 포함 | 837 PASS / 0 FAIL / ignored0 |
| 새 core fixture release | 4 / 0 / 0 |
| app no-default-features lib `local_runtime` | 36 / 0 / 0,2870 filtered |
| 상한 수정 전 최적화 meter debug/release | 각각14 / 0 / 0,66 raw/66 checked |
| 최종 상한 meter debug/release | 각각14 / 0 / 0,66 raw/66 checked |
| core wasm32 타입 검사 | exit0 |

core source와 새 fixture SHA는 이 실행들과 app 상한 변경 사이에 같다. app의 숫자·주석만 바뀐 뒤 전체 core를 반복 실행하지 않고 최종 meter와 WASM typing을 실행했다. 별도 독립 DEEP code review→qa-cli도 이후 실제 PASS했다. 아래 독립 검증 기록과 owning 종료 대기를 구분한다.

## 재현 환경과 명령

기존 cached Rust1.98.1 (`48a229ceaefd4985c50990b14116b6d856af0985`, LLVM22.1.8), Linux x86_64, pointer/Event/JudgeEvent/RuntimeReport8/144/136/344B다. 설치·다운로드 없이 이 checkout의 host CC/AR를 사용했다. 새 clone에서는 호환 도구를 준비한 환경에서 명령을 실행해야 하며 ignored 캐시/로그를 배포 파일로 간주하지 않는다.

```sh
source target/toolchain/env.sh
export RUSTUP_AUTO_INSTALL=0 CARGO_BUILD_JOBS=2 CARGO_NET_OFFLINE=true
export CC="$PWD/target/toolchain/host-cc.sh"
export AR="$PWD/target/toolchain/host/usr/bin/x86_64-linux-gnu-ar"
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS
unset CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS
unset CARGO_TARGET_X86_64_APPLE_DARWIN_RUSTFLAGS
cargo test -p beatkernel --locked --offline -j2 --no-fail-fast
cargo test -p beatkernel --test judge_refresh_routes --release --locked --offline -j2
cargo test -p beatkernel-bms-runtime --no-default-features --lib local_runtime --locked --offline -j2
cargo test -p beatkernel-bms-runtime --no-default-features --test runtime_input_allocations --locked --offline -j2 -- --nocapture --test-threads=1
cargo test -p beatkernel-bms-runtime --no-default-features --test runtime_input_allocations --locked --offline -j2 --release -- --nocapture --test-threads=1
cargo check -p beatkernel --target wasm32-unknown-unknown --locked --offline -j2
```

## Source와 실제 산출물의 정체성

- 최적화된 engine SHA256: `1b70cc1f73423b97b226d231a4c55bb67fb3ed25900375e83a972b83784fdd15`.
- 새 core fixture SHA256: `04766d5d57776c4670d8e07461f8579f672c439bba111e382e5004d1798e424c`.
- 최종 app meter SHA256: `9f99bcef5e22021180a5702ef2dd7a4c0304bf1fd443d9b0c6942012929fb305`.

실제 Cargo 후 native 실행파일은 이전 입력 계측의6f5bed91…에서 아래675d5cbd…로 바뀌었다. GUI 검증의 과거 c43 바이너리와도 다르다. `--no-default-features` 통합 테스트가 만든 현재 바이너리에 과거 GUI PASS를 이월하지 않는다. 기존 browser JS/WASM4개는 변하지 않았고 이 최적화를 포함하도록 재생성하지 않았다. core WASM 타입 검사는 현재 served browser package의 동작·성능 증거가 아니다.

| 실제 artifact | SHA256 | 바이트 |
| --- | --- | ---: |
| `app/web/pkg/beatkernel_bms_runtime.js` | `33733edc7cb07d63a7f79fb9c813706cf811b193230ef561a8c4c826f90fb89e` | 232,884 |
| `app/web/pkg/beatkernel_bms_runtime_bg.wasm` | `d2bdf298cd3aec4ee87b33e1e48fb389881f33468bee96062f52e23961a7891c` | 5,460,000 |
| `app/web/audio-pkg/beatkernel_bms_runtime.js` | `eaa1340083a7e6dc40b6cecbd8b9c888b8f07ac311ddf0d9d26116c0f849eb40` | 19,382 |
| `app/web/audio-pkg/beatkernel_bms_runtime_bg.wasm` | `52940c9236a5b531b819c58d111379e66ecf09c73494dfa73147b13fedd508a0` | 175,806 |
| `target/debug/beatkernel-bms-runtime` | `675d5cbd27d746af6bc95b2ae954e178556d597cf9f78e1ce7da279317c2c270` | 181,917,864 |

## 증거와 남은 의무

원래 기준선은 [입력 계측 기록](2026-10-16-runtime-input-allocations.md)과 `target/wf/runtime-input-alloc-ac002-14124/calibrated-baseline-and-budgets.json`, 독립 QA의 `target/wf/runtime-input-alloc-qa-cli-14125/{debug.log,release.log,invocations.json,evidence-verification.json}`다. 이번 원본은 ignored `target/wf/judge-refresh-ac002-14125/`의 각 stage raw log·invocations 및 before/final `post-route-measurements.json`에 보존한다. 실행 중 세 source identity가 고정됐고 최종 row의 모든 요청량은 상한 수정 전과 같다.

| 이번 root raw log, 위 디렉터리 기준 | SHA256 |
| --- | --- |
| `correctness/core-debug.log` | `b5abb0d15c8c860b0a9dfb081a340e53101cb770479acdd491d1298cb4976d08` |
| `correctness/core-routes-release.log` | `6a26d5774595102a01dbfd81173f5de61e943324268bc45014a39ab81735c5af` |
| `correctness/app-local-runtime.log` | `9537a0c7c8887cf1506ab48db2b473cffc81ad056d8de2abf1fc7271c5695475` |
| `before-budgets/meter-debug.log` | `e36502d361f9c811cf3cc93a4cbbb27a3d2ba53b4f485e4f50c40c74f5b0a265` |
| `before-budgets/meter-release.log` | `14945b5c54bc749a609435262abb083a2f5553ae48f8179ff678cb33ff56f85d` |
| `final-budgets/meter-debug.log` | `85588f69001d7856ba9da0428131b9b19c0f21440888d929ca8d47b58fe67031` |
| `final-budgets/meter-release.log` | `b596cc52d58d1a2607d7547bee85d6f1e100bcfe05397ec5dbeff6a18d9a168c` |
| `final-budgets/core-wasm.log` | `171d34cb9ddd19cd3b4ebb307b37be6367d984e57c6b74a6ef0bd254d6112b73` |

요청 수의 감소는 실제 입력 지연·CPU·RSS·세계 순위나 모든 플랫폼의 개선을 증명하지 않는다. System 내부와 caller callback, Native clock·다른 thread·lock/IO tracing·각 OS/물리 callback은 별도 미완료 의무다. nonempty 알고리즘은 source와 행동 회귀로 보존했으며 비어 있지 않은 모든 커스텀 callback의 elapsed-time 성능을 측정했다고 주장하지 않는다.

WBS13.06.01은 넓은 관측 경계가 남아W다. 전체136/489(27.81%)·오디오12/27·원래91/193과 전체 player Goal 미완료를 유지한다. 현재 child의 독립 정식 code review·qa-cli는 실제 PASS했으나 아래 자동 완료 기록 gate 때문에 종료는 미완료다.

## 독립 검증과 종료 대기

커밋 `a71bcd5d21d39faa0a93a4ce9e0cd9e13b0f4235`에서 fresh correctness와 contract/test discovery는 각각빈 배열이었다. 그 후 `code_review_judge_refresh_14125_c1`의 전체 DEEP 정식 검토가 FIX_NOW0/INVESTIGATE0/OPTIONAL0으로 실제 PASS했다. 정식 검토는 source3개와 문서4개, 호출 경로·66개 원본/상한·artifact 정체성을 독립 대조했다.

그 actual PASS 이후 `qa_cli_judge_refresh_14125`가 위6개 Cargo 명령을 순서대로 각각1회 실행해 모두 exit0으로 PASS했다. 전체 core837/0/ignored0, 새 release4/0, app local_runtime36/0/2870filtered, meter debug/release 각각14/0/66raw/66checked, core WASM typing0이다. 기존 교정 raw·기존 독립 QA·이번 상한 전후·새 QA의 모든66개 name/params/target/type/Requests와 literal 상한을 별도 parser로 재대조했다. 31개 감소·35개 동일·증가 없음, 교정9/public57과 단일13scalar·overflow·unknown 거부도 그대로 확인했다.

독립 QA 전후7개 tracked source/doc·HEAD와 실제5개 artifact hash/크기는 고정됐다. native675d5cbd…/181,917,864B와 browser4개가 위 표와 일치하며 현재 GUI/served WASM/물리 성능 PASS는 부여하지 않는다. 검증 깊이는 test-suite다. private core 변경이므로 새 CLI/help/UX가 없고 실제 public API를 테스트에서 실행했다.

최초 QA rustfmt 재귀 검사는 범위 밖 child `hazard_storage_fixtures.rs` 포맷 차이로 exit1이었다. 원본 `rustfmt.log`를 보존했으며 파일 수정 없이 `--config skip_children=true`의 실제3파일 scoped 검사를 실행해 exit0을 확인했다. 정적 parser의 `Requests::default()` arm 누락도 고쳐25arms/66cases를 모두 대조했다. 이 검사기 보정 때문에 Cargo 명령을 반복하지 않았다. diff/WBS/scoped format도 exit0이다.

로컬 ignored `target/wf/judge-refresh-qa-cli-14125/invocations.json`과 `evidence-verification.json`에 exact argv/env/compiler/exits/source·artifact identity, raw 대조와 검사기 실패 이력을 보존한다.

| 독립 QA raw log, 위 디렉터리 기준 | SHA256 |
| --- | --- |
| `core-debug.log` | `a8f61071a2884d963986a3be0c6ab2f40e915a946726a03364df1edec2e7c038` |
| `core-routes-release.log` | `dfc7fd86e266318e723be552909c7240213a0696e5ce5ad10abb78d08a0cc10f` |
| `app-local-runtime.log` | `a20097931a3c661f0f3f87f02ad1ec5d70db3da9bb520d5b2efdfa7183fb3450` |
| `meter-debug.log` | `740768b979e9e0271033d930069591408c36b4bca8e6d7096d4e3f0ce77483de` |
| `meter-release.log` | `c4333ae8c7a35bfc67de0bd1eb0441bf8a0c19ed3f629fcd9eec3c843b5e0e34` |
| `core-wasm.log` | `8d6ed13006f1ce4b52a7e35ff69c0933974995ecaadec051bd40aba3d7702994` |

실제 review PASS→QA PASS 뒤 한 번 owning `task_verify`는 필수 자동 review/QA 완료 기록 부재로 PENDING이었다. 현재 run의 `RECEIPTS.jsonl`은 없고 row0이다. direct final을 hook-owned 증거로 대체하지 않고 close하지 않는다. owner가 제공한 대기 조건으로 미완료 상태를 보존하며 전체 Goal은 활성이다. source/test 재실행·watcher 수리·receipt 수동 작성으로 종료 권한을 만들지 않는다.

다음 후속 후보는 Runtime의 첫 owned 판정 묶음을 빈 report에 복사하는 비용 제거다. 아직 구현·비용 감소 실측은 없으며 별도 계획에서 fanout·오류 accepted-prefix와 dense 묶음의 잔여 capacity를 함께 확인해야 한다.
