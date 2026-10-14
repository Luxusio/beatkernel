# Retained Source output replacement — 개발 중

실제 WASAPI의 SourceCounter와 일반 SourceFrame을 유지하면서, 일시정지한
플레이의 출력 설정을 바꾸고 같은 Mixer/Runtime/입력/capture로 재개하는 경로다.
[계약](../runtime/REQ__retained-source-output-replacement.md)과
[WBS 07.08.07](../kernel/REQ__bms-player-work-breakdown.md)을 따른다.
현재 전체 연결의 완료나 독립 최종 검증을 선언하지 않는다.

## 현재 구현과 커밋

- `f4c00398`: NativeOutputState의 실제 pending report 시작점까지 Source 이력을
  보존한다. practice pin과 함께 clamp하고 아직 clamped인 요청은 완료하지 않는다.
  불명확한 native admission은 상태를 보존한 채 재전송/재설정/추출을 격리한다.
- `9e11757d`: Source 조건부 종료 포트와 정적 Remix 전달을 추가한다. 원래
  native 좌표, 불투명 ticket, 요청 행렬, 오류 및 전체 소유권을 전달한다.
- `835cc813`: 원래 creation pin과 새 first-unsent basis를 분리하고, 실제
  Source 좌표로 native epoch/pause를 준비·검증·borrowed commit한다. 원래
  history/startup/gap/HOST floor를 보존하며 old held suffix만으로 승인하지 않는다.
- `cd4cc66a`: portable WASAPI worker는 fill과 clock-only 모두 같은 원자적 종료 결정을 거친다.
  성공한 양수 Release 뒤에만 prefix를 admit하며 동시 local/release 실패를 보존한다.
- `1d19c952`: complete WASAPI의 open/start/Stop/join/recovery와 원래 Counter
  descriptor를 연결한다. Legacy/complete owner를 구분하고 전체 오류와 quarantine을
  보존한다. cold history 준비는 실제 native geometry를 받은 제한된 메서드다.
- `72ac7c98`: 기존 conditional storage와 recovered/open/start lifecycle을 재사용해
  Source controller를 연결한다. 실제 새 descriptor와 두 fresh 관측으로 준비하고
  capability staging 뒤 borrowed atomic commit한다. Ready 실패는 owner와 hold를 보존한다.

## 실제 개발 검사

기존 `target/toolchain/env.sh`, host CC/AR, `CARGO_BUILD_JOBS=2`를 사용한다.

```sh
cargo test -p beatkernel-bms-runtime --lib --no-default-features --locked -j2
cargo test -p beatkernel-platform --lib --locked -j2 output_state::
cargo test -p beatkernel-platform --lib --locked -j2 source_history_retirement
cargo test -p beatkernel-platform --lib --locked -j2 wasapi_source_worker
cargo test -p beatkernel-platform --test wasapi_source_worker_allocations --locked -j2 wasapi_source_worker_external_allocation_calibration_and_exact_steps
cargo check -p beatkernel-bms-runtime --lib --no-default-features --features browser --target wasm32-unknown-unknown --locked -j2
```

최신 관측 결과는 app **2732 PASS / 0 FAIL / 6 ignored**, platform **411 PASS /
0 FAIL / 2 ignored**, worker14와 외부 heap 검사1 PASS다. Source controller21,
port/remix10·native14·pause5와 authority 회귀는 app 전체에 포함되므로 더하지 않는다.
Source와 기존 Target 실제 publication의 별도 외부 할당 검사도 각각1 PASS이며,
Source는 calibrated cold token copy 뒤 실제 held commit의 alloc/realloc0을 확인했다.
앞선 browser WASM check는 exit0이다. cached Windows platform lib/tests와 app lib
typing도 exit0이나 C-stub Rust 검사이며 native 실행 근거가 아니다.
외부 binary는 실제 production worker/state/history와 기존 System allocator를
사용한다. alloc/realloc/dealloc 각각의 실제 보정 후 success/refusal/authorized/
release-failure/quarantined retry 경계의 heap 호출0회를 확인했다.
Task Rust에 한정한 edition2021/default-style rustfmt 및 diff 검사도 통과했다.

초기 신규 fixture는 PCM byte 예산, 유효한 물리 PCM Hz, 이미 constructor가
거절하는 packet 경계, mutable producer 설정을 수정한 후 재실행했다. production
유효성 검사는 완화하지 않았다. 원래 실패를 포함한 로그는 ignored
`target/wf/retained-sourceframe-output-replacement/foundations/`에 있다. 로그는
다른 clone에 없을 수 있으므로 위 명령과 테스트 source로 다시 확인한다.

## 남은 연결과 검증 경계

Windows complete open/start/Stop/join/recovery는 기존 Legacy Mixer/remix API와
구분한다. 최초 native 크기의 history 준비도 제한된 cold 메서드로만 수행하고
complete owner를 Mixer로 평탄화하지 않는다. 실제 clock frequency는 새 장치에서
얻는다. 새로 생성한 descriptor와 Ready prefill은 playback progress가 아니다.

Controller 개발 검사는 통과했고 Owner와 canonical adapter는 작성 후 검사 중이다.
typed backend envelope 안의 packet/identity/timing 실패를 raw IO와 구분하는 경합
검사도 포함한다. 다음 typed UI, Windows solo/local hook과 실제 공통 pump를 연결해야 한다.
전체 native owner/PCM/producer/capture 연동 보존, 실패·취소·명시 retry와 독립 review 뒤
CLI/desktop QA가 남는다.
Windows typing은 C-stub에 의한 Rust 형 검사이며 COM/driver 실행이 아니다.
현재 host의 Windows 장치 실행을 소프트웨어 검사로 대체하지 않는다.
WASAPI exclusive·ASIO·CoreAudio, 나머지 rate/encoding·cross-backend·browser·
물리/성능/출시 범위는 원래 Goal/WBS에 남는다.
