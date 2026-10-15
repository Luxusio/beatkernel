# ASIO retained Source output — 소프트웨어 검증 결과

ASIO의 정확한 heterogeneous planar 출력 상태, 남은 원본 PCM과 제출 prefix,
original native interval을 기존 Source Owner·typed Output UI·solo/local pump에
연결하는 작업이다. 사전 독립 설계 검토 PASS 뒤 REQ/ADR을 먼저 기록했다.
독립 코드·보안 검토와 PLAN이 허용한 가용 소프트웨어 QA를 통과했다. 원래 BMS
player Goal과 WBS07.08.07.14의 실제 SDK/driver acceptance 요구는 유지한다.
Hook-owned 완료 gate는 실제 검토·QA 결과와 별도이며 전체 플레이어 완료를 뜻하지
않는다. 아래 개발 검사는 중간 시점 기록이고 현재 결과는 마지막 독립 검토·QA 절을 따른다.
설정 변경은 일시정지·출력 정지·재설정·같은 플레이 재개의 순서이며 그동안 소리는
멈춘다. PCM 중복/누락과 재개 싱크 보존이 목표이고, 무중단 재생이나 물리적인
무잡음 전환이 입증됐다는 뜻은 아니다.

기준 소스는 `a12ad712d4ff0be67e6c772a9bbfcdb24f7c5f54`다. 변경 전
Rust-only Windows GNU ASIO cfg check는 기존 `target/xcheck-asio-rust-only-12e0c`에
fingerprint를 쓰다가 I/O error5로 종료됐다. 이는 typing PASS도 소스 타입 오류
판정도 아니다. 실행 handle은 종료/소실했고 cargo/rustc가 남아 있지 않음을 확인했다.
후속 변경 검증에서 실제 결과를 별도로 기록한다. SDK feature build를 실행하거나
SDK/MSVC/C++ ABI를 입증한 것이 아니며 SDK/도구/driver를 설치하지 않는다.

## 개발 검사 — 첫 기초 묶음

`3a8689c9`의 exact planar 상태, `a4c51f24`의 original report/delivered 구간,
`7e5124d9`의 typed cohort cleanup을 기능별로 커밋했다. 실제 플랫폼 library는
487 PASS/실패0/ignored2, 새 공개 delivery integration은9 PASS, cohort cleanup은
5 PASS/실패0/ignored1이다. 마지막 ignored는 기존 TLS 환경이 필요한 실제 QUIC
owner 종료 검사이며 통과로 세지 않는다. compiled platform test binary의 validator26과
legacy ASIO21은 위 library와 중복이다. WBS 도구50개 검사도 통과했다. 새 PCM
fixture2개의 초기 실패는 독립적으로 시작한 키음의 sample phase 기대값 오류였고,
수정 후 전체 플랫폼 library를 재실행했다. 전체 소스의 최종 독립 리뷰·QA는 남아 있다.

개발 로그는 이 컴퓨터의 ignored `/tmp/beatkernel-asio-platform-lib-14038.log`,
`/tmp/beatkernel-asio-delivery-tests-14038.log`,
`/tmp/beatkernel-asio-finalizer-tests-14038.log`다. 재현 명령은 캐시 toolchain 환경에서
`cargo test --locked --offline -j2 -p beatkernel-platform --lib`,
`cargo test --locked --offline -j2 -p beatkernel-platform --test asio_delivery_presentation`,
`cargo test --locked --offline -j2 -p beatkernel-bms-runtime --lib native_cohort_setup::typed_cleanup_fixtures`다.

## 개발 검사 — callback·공통 앱·native 연결

`ce894163`의 portable production callback20개를 포함해 플랫폼 library507 PASS/
실패0/ignored2를 실행했다. 일반 Linux dependency 빌드에서 빠진 cfg를 발견해
Windows ASIO 또는 test 조건으로 수정했다. 앱 전체 default library는 실제로
3283 PASS/실패0/ignored9다. 새 shared queue10·실제 controller/Owner6·원본
coordinate/end/scalar 경계8·정확한 cold layout/matrix4 검사가 포함되며 별도 합산하지 않는다.
`79f22f4b`·`f91da5c4`·`074b8bab`에 이 공통 연결을 나눠 커밋했다.

실제 `bridge_dispatch.h`를 include하는 C++ fixture의6개 함수를 assertions가
활성화된 채 컴파일·실행해 exit0을 확인했다. 소유 SDK 형식을 대체한 header는 없다.
캐시 g++13은 기존 `target/toolchain/env.sh`의 LD 경로가 필요했다. 처음 환경을
빠뜨려 libisl 로드가 실패했으며 기존 env를 적용한 뒤 통과했다; 설치는 하지 않았다.
빌드 명령은 `source target/toolchain/env.sh` 뒤
`target/toolchain/host/usr/bin/x86_64-linux-gnu-g++-13 --sysroot=$PWD/target/toolchain/host -B$PWD/target/toolchain/host/usr/bin -std=c++17 -Wall -Wextra -pedantic -O2 crates/beatkernel-platform/tests/asio_callback_dispatch.cpp -o /tmp/beatkernel-asio-callback-dispatch-14038`이고,
같은 환경에서 생성된 executable을 실행한다. build/run 로그는
`/tmp/beatkernel-asio-cpp-build-retry-14038.log`·`/tmp/beatkernel-asio-cpp-run-14038.log`다.

Native platform all-target Rust-only 검사도 exit0이다. 실제 Cargo SDK feature는
끄고 Windows GNU target에만 Rust cfg를 주입했으며, C++ SDK build/link/MSVC ABI를
검증하지 않았다. native lifecycle 신규16개는 **typed-only**, 실행 PASS가 아니다.
문서 주석4곳·fixture mut2곳과 source-included fixture의 로컬 타입 import 충돌을
고쳐 최종 검사했다. 후자의 실제 host external allocator binary128 PASS/실패0을
실행했으며 library와 중복된 검사는 고유 coverage에 더하지 않는다. 로그는
`/tmp/beatkernel-asio-native-rust-only-final-14038.log`·
`/tmp/beatkernel-asio-source-external-14038.log`·
`/tmp/beatkernel-asio-common-app-retry-14038.log`다.

현재 device-lab owned/backends 조회는 broker 연결/소유 충돌로 실패해 실행 가능한
소유 Windows runner를 확인하지 못했다. 장치나 외부 runtime 상태를 변경하지 않았다.
실제 driver 존재 여부를 확인하지 못한 결과를 ‘장치 없음’으로 해석하지 않는다.

## 개발 검사 — native·canonical adapter 결합

`90f3e5ec`에 complete native lifecycle과 canonical Source adapter를 함께
커밋했다. 실제 cleanup `Arc`·native fault·telemetry 실패가 먼저 반환되고,
건강한 출력의 승인 경합만 좁은 `RecoveryUnavailable`로 처리한다. Authorized와
native/cleanup 실패가 동시에 있는 실제 lifecycle fixture2개를 추가해 기존
정상 승인·잘못된 basis/rate 반대 경로와 함께 형 검사했다. native lifecycle은
총18개, adapter fixture는총17개이며 이 환경에서는 모두 **typed-only**다.

`cargo check --locked --offline -j2 --target x86_64-pc-windows-gnu -p beatkernel-platform -p beatkernel-bms-runtime --all-targets`가
기존 Rust-only cfg/cache 환경에서 exit0이다. 로그는
`/tmp/beatkernel-asio-complete-rust-only-14038.log`다. Target별 Rust cfg만
사용하고 실제 Cargo SDK feature를 켜지 않았으므로 SDK C++/link/MSVC ABI나
driver 실행 PASS로 해석하지 않는다. Windows UI·solo/local은 이 dependency
검사 뒤 source/test pair로 개발 중이다.

## 개발 검사 — Windows 플레이 연결

`d53b777d`는 기존 Source owner·typed UI·solo/local pump에 concrete ASIO
complete variant를 연결한다. 초기 control probe와 request/pending/success 출력이
HWND를 보존하며, native release가 미입증이면 보수적으로 창을 유지한다. local의
오류·정리·경로 준비 실패는 공통 typed finalizer로 전달해 뒤의 capture도 시도한다.

실제 host Windows bin 전체는91 PASS/실패0/ignored0이다. 신규 portable12개가
포함되며 static composition1개는 Windows cfg에서 형 검사만 했다. 이 테스트가
실제 HWND·native startup·local run caller를 실행했다는 뜻은 아니다. 특히
local cleanup 우선순위와 retry/member-destination 누적 caller는 정적으로
추적했으며 shared finalizer 테스트는 그 caller를 우회하므로 직접 실행 증거로
확대하지 않는다. 실제 native caller의 실패 경로 검사는 별도로 남는다.

동결 후 platform/app all-target Windows Rust-only check도 exit0이다. 로그는
`/tmp/beatkernel-asio-windows-bin-14038.log`·
`/tmp/beatkernel-asio-windows-composition-14038.log`다. 실행 명령은 캐시 host
환경의 `cargo test --locked --offline -j2 -p beatkernel-bms-runtime --bin windows_bms`와
앞 절의 같은 Windows GNU all-target check다. 최종 독립 리뷰·QA는 아직 진행 전이다.

## 독립 리뷰 수정

`7b3779a7`의 첫 DEEP 정식 code review는2건으로 FAIL, 별도 security review는
PASS0이었다. `a56e039b`는 history 없이 생성한 pending PCM을 history 준비 전에
거절해 원래 owner와 반복 재시도를 보존한다. `8ee68fb1`는 초기 probe·pre-transfer
실패의 명시적 close 결과를 기존 typed carrier에 담아 원본 오류·cleanup 오류와
실제 guard를 보존한다. 새 오류 프레임워크는 추가하지 않았다. 일반적인 guard
폐기는 Result 채널이 없으므로 원본 cleanup 진단을 출력하고 HWND를 보수적으로
유지한다.

수정 후 실제 host Windows bin94 PASS/실패0/ignored0, platform/app all-target
Windows Rust-only check exit0이다. 새 mapper 회귀3개를 실행했고, 새 native
history 회귀3개를 포함한 lifecycle21개는 **typed-only**다. 성공한 빈 history
준비·기존 history pending 재사용·실패 후 원본 PCM/queue와 반복 거절을 검사하도록
작성했다. 로그는 `/tmp/beatkernel-asio-formal-fix-windows-bin-14038.log`·
`/tmp/beatkernel-asio-formal-fix-rust-only-14038.log`다. 재검토와 독립 QA는 남아 있다.

## 독립 재검토 수정 — 동일 callback의 시계 재매핑

`0632e06b`의 두 번째 DEEP code review는1건으로 FAIL, 별도 security review는
PASS0이었다. 새 multimedia anchor가 같은 실제 callback을 다른 HOST 구간으로
매핑하면 동일 Source revision의 coordinate가 달라져 정상 교체도 거절됐다.
`272b564f`는 기존 고정 두 슬롯과 latest 원본 관측을 재사용한다. 현재 들어온
delivery·Source facts·식별자·domain·HOST 형식은 검증하면서, 동일 callback에는
처음의 coordinate·HOST 상한·age를 유지한다. 새 캐시·필드·adapter API는 없다.
시각 재매핑만으로 AwaitingHostProgress가 진행 상태로 승격되지 않으며,
SuppliedPair의 동일 revision 엄격성도 유지한다.

새 queue/controller 회귀는 총23개다. 기존 equal-midpoint 검사가 같은 callback의
HOST/revision 변경을 실제 진행으로 기대해 첫 실행은278 PASS/1 FAIL이었다.
독립 작성자가 그 기대를 수정하고, 반복 재매핑은 계속 대기하며 실제 다음 callback만
원래 HOST 상한 뒤에 진행함을 확인하도록 강화했다. 최종 실제 실행은
`cargo test -p beatkernel-bms-runtime --lib gameplay::output::application --locked --offline -j2`로
279 PASS/실패0/ignored0, exit0이다. 23개는279개에 포함되며 중복 합산하지 않는다.
로그는 `/tmp/beatkernel-asio-renewed-original-host-14038.log`·
`/tmp/beatkernel-asio-renewed-original-host-final-14038.log`다. 새 회귀의 변경 전 RED는
실행하지 않았으므로 RED 증거로 주장하지 않는다. 최종 독립 재검토와 QA는 남아 있다.

## 최종 독립 검토·소프트웨어 QA

소스 동결 HEAD `1a1aa5b018c5700395d86e3e9d9c7baae6f4c489`에서 세 번째
독립 DEEP code review와 별도 security review가 각각 실제 PASS0을 반환한 뒤,
단일 독립 qa-cli가 아래 matrix를 실행하고 실제 PASS를 반환했다. CODE 검토는
발견 예산을 모두 사용한 뒤 새 hunter 없이 전체53개 경로와 이전3건의 수정 근거를
독립적으로 확인했다. 검증 중 tracked source 변경은 없었다. 이 결과는 PLAN이
허용한 소프트웨어 범위이며 authentic SDK·Windows/driver·음향 acceptance가 아니다.

| 실행/검사 | 결과 | 로그 |
|---|---|---|
| Core lib/integration + doctest | 733 +16 PASS, 실패0; 기존749 기준 유지 | `/tmp/qa-asio-core-14038.log`, `/tmp/qa-asio-core-doc-14038.log` |
| Core examples 검사 | 별도1 PASS/실패0 | `/tmp/qa-asio-core-examples-14038.log` |
| Platform lib/integration | lib507/실패0/ignored2, integration285/실패0 | `/tmp/qa-asio-platform-14038.log` |
| App desktop,webtransport | lib3319/실패0/ignored9, bins670/실패0/ignored17, Source allocation1/실패0 | `/tmp/qa-asio-app-featureful-14038.log` |
| Host no-default bins / browser WASM | 각각 check exit0 | `/tmp/qa-asio-host-bins-14038.log`, `/tmp/qa-asio-wasm-14038.log` |
| Windows/macOS 일반 foreign typing | 각각 exit0 | `/tmp/qa-asio-native-14038-windows.log`, `/tmp/qa-asio-native-14038-macos.log` |
| Windows ASIO-cfg alltargets | Rust-only check exit0 | `/tmp/qa-asio-native-14038-asio-cfg.log` |
| 실제 production C++ dispatch | assertions 활성화6개 함수 compile/run exit0 | `/tmp/qa-asio-portable-14038-cpp.log` |
| Source Clippy lib/bins | exit0, 기존 경고 유지 | `/tmp/qa-asio-clippy-14038.log` |
| 공개 CLI | 19/19 예상 exit·출력 일치 | `/tmp/qa-asio-cli-14038.log`, `/tmp/qa-asio-cli-14038.jsonl` |
| WBS / 로컬 링크 / whitespace | 50 PASS, 8개 문서154링크 누락0, diff exit0 | `/tmp/qa-asio-portable-14038-wbs.log`, `/tmp/qa-asio-portable-14038-links.log`, `/tmp/qa-asio-portable-14038-diff.log` |

숫자를 고유 coverage 합계로 더하지 않는다. Platform integration에는 format23·
ASIO delivery9·library와 중복되는 external allocator128이 포함된다. App에는
queue15·실제 controller8·delivery8·typed finalizer5가 포함된다. 새 Windows
portable15개는 main/Windows bin에서 각각 실행되어 고유30개가 아니다. Native
lifecycle21·canonical adapter17·Windows gated composition1은 **typed-only**다.
실제 QUIC owner 종료1개는 CERT/KEY 환경이 없어 ignored이며 통과로 세지 않는다.

재현에는 기존 `target/toolchain/env.sh`, `RUSTUP_AUTO_INSTALL=0`, cached
host CC/AR와 jobs2를 사용한다. 실제 주요 명령은 다음과 같다.

```text
cargo test -p beatkernel --lib --tests --locked --offline -j2
cargo test -p beatkernel --doc --locked --offline -j2
cargo test -p beatkernel --examples --locked --offline -j2
cargo test -p beatkernel-platform --lib --tests --locked --offline -j2
cargo test -p beatkernel-bms-runtime --features desktop,webtransport --lib --bins --test coordinate_source_replacement_allocations --locked --offline -j2
cargo check -p beatkernel-bms-runtime --bins --no-default-features --locked --offline -j2
cargo check -p beatkernel-bms-runtime --target wasm32-unknown-unknown --lib --no-default-features --features browser --locked --offline -j2
cargo clippy --workspace --lib --bins --features desktop,webtransport --locked --offline -j2
cargo build -p beatkernel --example audio --locked --offline -j2
cargo build -p beatkernel-platform --example windows_audio --locked --offline -j2
cargo build -p beatkernel-bms-runtime --bin windows_bms --features desktop,webtransport --locked --offline -j2
```

Foreign 검사는 기존 `xcheck.sh windows|macos`의 platform/app lib/tests/bins
no-default typing과 앞 절의 별도 ASIO-cfg alltargets 명령이다. C stubs와 target별
Rust cfg를 사용했고 실제 Cargo SDK feature는 OFF다. 정확한 command/exit 목록은
`/tmp/qa-asio-host-tail-14038.json`과 각 foreign 로그에 있다. C++ 명령은 앞 절과
같이 기존 env를 적용해 actual production header fixture를 compile/run했다.

공개 CLI의 help·실제 메모리 PCM fixture·invalid·중복 옵션·동시 help를 실행했다.
실제 fixture 출력은 `frames=8 commands_applied=3 partition_invariance=PASS`와
`frames=4 literal_check=PASS`다. Linux의 Windows 전용 CLI는 유효한 ASIO 인자를
parse한 뒤 `native playback requires Windows`로 거절한다. 이를 Windows의
SDK-disabled native guard 실행으로 확대하지 않는다. SDK-disabled selector/UI의
상관된 단일 거절은 실제 portable bin tests에서 확인했으며 HWND/local native
caller의 실패 경로는 static-only다.

실제 rustfmt42경로의 raw exit1 중4개는 base와 HEAD의 formatter 변경 내용이
같았다: `app/src/asio_replay.rs`, `app/src/gameplay/output/adapters/observation.rs`,
`crates/beatkernel-platform/src/audio/asio/calibration.rs`,
`crates/beatkernel-platform/src/audio/presentation/validation.rs`다. 나머지38개 명시
경로 check는 exit0이며 새 포맷 회귀는 없다. `/tmp/qa-asio-portable-14038-format.log`,
`/tmp/qa-asio-portable-14038-format-baseline.log`, `/tmp/qa-asio-portable-14038-format-scoped.log`에
기록했다. 전체 formatter clean이나 strict Clippy clean을 주장하지 않는다.

기존 note freshness 검사도53개 변경 경로에 대해0개 invalidation/exit0이다.
금지된 `doc/HANDOFF.md`와 protected task 디렉터리를 walk에서 제외했고 실제 파일을
읽거나 변경하지 않았다. `/tmp/beatkernel-asio-note-freshness-14038.log`를 따른다.
동일 callback 재매핑·history 없는 pending·cleanup 오류 보존 규칙은 각 REQ와
커밋된 회귀에 반영했다. 기존 C++ 환경 gotcha는 앞 절의 재현 명령에 기록했으며,
동일 검사를 감싸는 추가 shell 테스트나 설치/SDK 변경은 추가하지 않았다.

WBS는 .14.01–.06 V, .14.07 W, 실제 SDK/driver .14.08 E와 전체136/489를
유지한다. 완료 표시는 hook-owned 최종 gate를 따르며 실제 리뷰/QA PASS 자체를
그 기록의 대체물로 쓰지 않는다. 전체 BMS player Goal은 계속 미완료다.

## Known ceiling

Authentic MSVC/ASIO SDK build와 설치된 실제 driver/HWND/callback/음향 동작은
아직 미입증이다. Portable production seam 실행과 foreign Rust typing을 이
native acceptance로 확대하지 않는다. Callback retired와 성공한 native release는
별도 증거이며, 실패 시 소비된 handle의 stop 반복을 release 성공으로 취급하지 않는다.
Hook-owned 최종 완료 증거도 실제 리뷰·QA와 별도 gate다.

Known ceiling: 실제 ASIO 드라이버 동작은 미검증 — native SDK·드라이버 검증 환경에서 확인 필요

Known ceiling: authentic SDK/MSVC 및 실제 드라이버 실행 미검증 — 해당 환경에서 검증할 때 해제

Known ceiling: Windows execution unverified — validate with coordinator typing/tests and native acceptance.
