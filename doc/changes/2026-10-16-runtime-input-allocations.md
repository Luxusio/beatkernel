# 실제 입력 경계의 할당 기준선 — 2026-10-16

## 범위와 재현

생산 코드는 변경하지 않았다. 기준 revision은 `9e09c1e54e347f25a81bbbca5ef00a31cc5d8b40`이며, 새 `app/tests/runtime_input_allocations.rs`가 실제 JudgeEngine/Runtime/SoloRuntime/RuntimeGroup 공개 입력 호출을 계측한다. fixture 준비·완료 prefix 처리·queue 채우기/consumer 해제·clock script·결과 검사·로그·반환 report 해제는 측정 밖이다. 호출 안에서 소비된 owned input의 해제는 측정에 포함된다.

Cached Rust1.98.1 (`48a229ceaefd4985c50990b14116b6d856af0985`, LLVM22.1.8), `x86_64-unknown-linux-gnu`, no-default-features, offline/locked/j2를 사용했다. pointer/Event/JudgeEvent/RuntimeReport 크기는 각각8/144/136/344바이트다. `target/toolchain/env.sh`의 기존 host CC/AR를 사용했다.

```sh
source target/toolchain/env.sh
export RUSTUP_AUTO_INSTALL=0 CARGO_BUILD_JOBS=2 CARGO_NET_OFFLINE=true
export CC="$PWD/target/toolchain/host-cc.sh"
export AR="$PWD/target/toolchain/host/usr/bin/x86_64-linux-gnu-ar"
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS
unset CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS
unset CARGO_TARGET_X86_64_APPLE_DARWIN_RUSTFLAGS
cargo test -p beatkernel-bms-runtime --no-default-features \
  --test runtime_input_allocations --locked --offline -j2 \
  -- --nocapture --test-threads=1
cargo test -p beatkernel-bms-runtime --no-default-features \
  --test runtime_input_allocations --locked --offline -j2 --release \
  -- --nocapture --test-threads=1
```

## 실제 측정과 고정 상한

교정 완료 baseline은 debug/release 각각13PASS·0FAIL·ignored0이었다. 고정 상한과 단일 초과 필드/unknown 거부 대조를 넣은 최종 root Verify는 각각14PASS·0FAIL·ignored0이며66개 case 모두 상한 검사를 통과했다. 최종 두 profile의 모든 Requests 값이 교정 baseline과 같음을 별도 parser로 대조했다.

각 profile은 교정9개·실제 public 호출57개, 총66개 이름 있는 측정 row를 출력했다. baseline 두 profile의 모든 요청 계수가 일치했다. source 안의 정적 case별 상한은 이 실제 baseline에서 고정하며 현재 실행의 결과를 상한으로 사용하지 않는다. 미등록 case는 실패한다. alloc/zeroed/realloc/dealloc 각각 attempt/success/requested bytes와 realloc old bytes를 구분하며 overflow는 거부한다. 성공 해제는 반환값 없는 System dealloc 전달을 뜻한다.

상한의 경험적 범위는 Linux x86_64/64bit다. 다른 target도 같은 의미·교정·측정 검사를 실행하지만 상한 미검증을 명시한다. 새 compiler나 profile을 이유로 지원 host의 상한 검사를 우회하지 않는다. 다른 OS/architecture 비용의 증거로 확대하지 않는다.

| 실제 case | alloc 회/요청 B | dealloc 회/요청 B |
| --- | ---: | ---: |
| Judge legacy/observed fresh hit | 6 / 1,008 | 4 / 348 |
| Judge reused association / 4096 completed prefix | 5 / 892 | 4 / 348 |
| Runtime legacy fresh hit | 10 / 2,372 | 5 / 892 |
| Runtime observed fresh hit | 11 / 2,628 | 5 / 892 |
| Runtime reused association / 4096 completed prefix | 8 / 2,300 | 5 / 892 |
| Runtime observed two-binding fanout | 16 / 3,520 | 10 / 1,784 |
| Runtime queue full/disconnected | 11 / 2,660 | 5 / 892 |
| Group 1/2/4/64 first/last | 11 / 2,724 | 5 / 892 |
| Solo admitted | 11 / 2,724 | 6 / 1,244 |
| Solo full/disconnected | 11 / 2,756 | 6 / 1,244 |
| Group full/disconnected | 11 / 2,756 | 5 / 892 |
| Runtime owned RawHid returned | 1 / 84 | 0 / 0 |
| Group unassigned RawHid consumed | 0 / 0 | 1 / 257 |
| Runtime preflight refusal / Group unassigned button / poisoned followup | 0 / 0 | 0 / 0 |

이 표의 business row는 alloc_zeroed/realloc 요청이0이었다. 교정은 alloc37B, alloc_zeroed61B, realloc37→113B, dealloc113B를 별도로 실제 실행한다. 이 수치는 outward owned report 비용을 포함한 **할당 요청량**이며 live/peak/RSS·CPU·지연 시간이 아니다. 64명 첫/끝의 할당 수가 같아도 실제 route scan은 선형이므로 O(1) CPU라고 주장하지 않는다. production 최적화나 다른 플레이어 대비 향상은 이 변경에서 측정하지 않았다.

## 의미·소유권 대조

- Judge legacy/observed hit, unmatched/held/repeat/up와 시간 역행, 실제0/1/4096개 완료 prefix를 검사한다. Judge에는 영구 report history Vec가 없으므로 외부 Vec 길이를 이력 부하로 꾸미지 않았다.
- Runtime은 원본 metadata/source/sequence, binding fanout·disposition, song/audio point, domain/작은 sequence/host-time 거부 후 상태·queue를 대조한다. 같은 sequence는 유효하므로 regression에는 실제 더 작은 값을 사용한다.
- admitted/full/disconnected는 실제 queue의 sentinel·FIFO·원본 command와 이미 완료된 판정을 검사한다. Solo는 partial report의 Ok, Group은 ReportedFailure이며 이후 poison이 유지되는 서로 다른 계약을 보존한다.
- 자동 single-player는 임의 source를 받아들이며 exact group은 고유 양수 player/distinct devices로1/2/4/64인 첫/끝/미배정 경로를 실행한다. owner 하나만 처리하고 다른 member 상태는 보존된다.
- 256B RawHid는 Runtime report가 원본 payload를 보유한다. 미배정 Group이 소비하는257B payload의 해제는 호출 안에 포함된다.
- 기본 측정은 RuntimeProcessingClock::Disabled다. capacity0만으로 Native Instant가 꺼진다고 가정하지 않는다. 별도 External clock은 정상·거부에서 각각2회 호출과13/17ns 처리 delta를 검사한다. allocating mapper는 실제 map2/quality2 호출·인자와 동일 업무 전이의 normalized control을 대조하며 alloc/dealloc 각각2회·62B 추가를 확인한다.

## 실패 원본과 교정

첫 debug 실행은12PASS/1FAIL이었다. 신규 audio oracle가 setup sentinel enqueue의 누적 audio_commands를 빠뜨렸다. 실제 setup을 유지하고 측정 전 counter snapshot에 호출 delta를 더하도록 고쳤다. 해당 debug 재검사1/0은 통과했다.

첫 release는12PASS/1FAIL이었다. nested positive calibration의 null 검사와 free만 남은29B 할당을 optimizer가 제거했다. 실제 pointer를 black_box로 관측하고 유효 offset0/28의 volatile 내용 검사를 유지해 교정했다. 정확한 alloc1/29B oracle는 완화하지 않았다. 수정 후 coherent debug/release 전체13/0 기준선을 다시 확보했다. 최초 실패 로그는 보존하며 이를 production 회귀나 무할당 성과로 해석하지 않는다.

## 효과 경계와 미완료 의무

소스 감사의 hot path는 Runtime normalize/prepare→BindingMap→Judge→audio admission→telemetry, Group의 owner swap과 한 Runtime 호출, Solo의 단일 report 추출이다. prepared audio SPSC admission은 Full/Disconnected를 즉시 반환한다. Mixer/native output callback은 이번 측정 범위 밖이다.

Judge candidate/result/route 및 held/dispatch storage, Runtime binding/report/audio/disposition/source association, Group report Vec는 실제 할당 후보와 관측값의 원인이다. OwnerGuard는 mutex가 아니라 swap/Drop이며 telemetry summary clone/sort는 측정 밖이다. 주입한 mapper/resolver/policy/active interaction은 caller code이고 System allocator 내부도 별도 효과 경계다.

기존 strace/ltrace/bpftrace/perf는 이 환경에 없어 runtime lock/IO tracing을 실행하지 않았다. 설치하지 않았다. 소스 경로에서 직접 잠금/파일 호출을 못 찾았다는 사실이나 Disabled clock은 **System·임의 callback·Native timer·다른 thread·각 OS/물리 callback의 lock/IO 부재 증명**이 아니다. WBS13.06.01은 넓은 경계의 관측이 남아 W를 유지한다. 전체136/489(27.81%), 오디오12/27, 원래91/193과 전체 BMS player Goal 미완료를 유지한다.

## 증거와 독립 검증

생산 tracked source가 같다는 사실과 실행파일 동일성은 구분한다. 추가 실제 SHA 대조에서 기존 browser JS/WASM4개는 그대로였지만 현재 `target/debug/beatkernel-bms-runtime`은 `6f5bed91dcae39f238f0667fe0fa7225ff7a9c79c604dd75bb789f17a3fa8b30`(181,920,776B)로 이전 GUI 검증의 `c43a8790…`와 달랐다. 이번 통합 테스트 Cargo 명령은 `--no-default-features`이며 test 컴파일 과정에서 app binary도 생성될 수 있다. 이전 desktop GUI 결과는 원래 c43 바이너리의 과거 증거로 보존하고 현재 실행파일의 GUI PASS로 이월하지 않는다. 이 작업에서 GUI 재빌드나 실제 native 장치 검증을 추가하지 않았다. 실제5개 artifact 상태는 로컬 `current-product-identity.json`에 기록했다.

기준선 source SHA256: `30b4854e19bd3ddcb3ae8d9a0f4331f8efccfea607cd425377cf12736c986093`.
debug log SHA256: `9f61de950401a0556af0ab0eed42ecbf323131bab123e9e68bd5e6fe529a5caf`.
release log SHA256: `735ec4a24f53c510b4a1b0b63d61122fb32188f51b064c75decd706410e2fba2`.

로컬 ignored `target/wf/runtime-input-alloc-ac002-14124/`에 첫 실패·교정 baseline·최종 실행 로그, compiler/argv/source identity 및 `calibrated-baseline-and-budgets.json`을 보존한다. 정적 상한과 fixture는 test source에 포함된다. 위 환경 설정은 이 checkout의 기존 ignored toolchain 캐시용이다. 새 clone에서는 호환 Rust와 host CC/AR를 별도 준비한 환경에서 Cargo 명령을 실행해야 하며, 이 작업에서 도구를 설치하거나 내려받지는 않았다. ignored 원본 로그가 배포 파일에 포함된다고 주장하지 않는다.

최종 root test source SHA256: `33b57d357cf710fbaf60921886b16dd4083c442e49a2cc0410e13c26e9d858d6`.
최종 debug log SHA256: `5723d684c4fbe269bbba2d02c75f6e8e6f2c7dbbfb598a43a1f86db16ffca86b`.
최종 release log SHA256: `06f63ca4be1923c1e9eeff1657f920d3b11e18fdef6233a5849d36de79912d3b`.

독립 `code_review_input_alloc_14125_c1`의 전체 DEEP 검토(both discovery, 확정 지적0)가 PASS한 뒤 `qa_cli_input_alloc_14125`가 커밋4c1c9f00에서 실제 scoped suite를 실행해 PASS했다. debug/release를 순서대로 각각1회 실행했고 각각14/0·ignored0,66개 raw/66개 상한 검사를 확인했다. case 이름·params·target/type metadata와 모든 Requests가 교정 raw baseline·JSON 및 Rust 고정 상한과 일치했다. 단일13 scalar 초과·overflow·unknown 거부도 두 profile에서 실제 실행했다. scoped fmt/diff/WBS 검사0, source SHA33b57d…858d6와 tracked/index 상태를 유지했다. 새 전체 Node/Rust/WASM/GUI/foreign 검사를 이 test-only delta의 근거로 추가하지 않았다.

독립 QA 로그 SHA256은 debug `baf31e4137fb52849cb34c7a7d38ab71ae2a825be7672f6f84d32145d12e0dc1`, release `428cd0df0cbc256c4bb117d8ba490485c054bf23f6090ba673f21ec7dbf03293`다. 로컬 ignored `target/wf/runtime-input-alloc-qa-cli-14125/`의 `invocations.json`, `evidence-verification.json`, `post-cargo-product-identity.json`과 raw log에 보존한다. 준비 Git ownership 검사128과 ignored evidence parser의 key 가정1은 준비/검사기 오류로 구분해 보존했고, 기능 검사를 반복하지 않았다. QA 후4browser와 현재 native6f의 실제 hash는 QA 전과 같았다.

실제 code review→qa-cli PASS 뒤 한 번 owning `task_verify`는 필수 자동 완료 기록 부재로 PENDING이었다. 현재 run의 receipt 파일/행은 없으며 직접 agent final을 hook-owned 완료 기록으로 대체하지 않는다. 실제 소프트웨어 PASS와 태스크 종료 권한을 구분해 close하지 않으며, owner의 기록 대기 조건에 따라 미완료로 보존한다. 전체 Goal은 활성이고 다음 `TASK__judge-refresh-empty-route`는 기존 임시 route 할당의 실제 감소를 검증할 별도 작업으로 대기한다. 아직 최적화 구현이나 감소 수치를 이 기록에 추가하지 않는다.
