# 실제 리플레이 owner의 고정 배속 연결

상태: 구현·개발 검사 완료, 독립 review/QA 대기. 기준 HEAD 5ed217c.
이 작업은 공통 StepReplay, native replay CLI/pause와 browser replay setup에
동일한 고정 유리수 배속을 전달하고, 실제 출력 presentation을 원본 곡 시간으로
변환한다. 요구 계약은 [실시간 배속](../kernel/REQ__realtime-replay-speed.md)이다.

계획 독립 검토에서 native finite cap-before-narrow와 quantized/zero output
종료 근거를 보완했다. 배속 숫자 모델, browser 숫자 검증, native finite
frontier는 각각 독립 구현·테스트 작성자 쌍으로 작성했다.

첫 묶음 개발 검사: format 이후 공통 ReplaySpeed 8개, 실제 native finite
frontier 6개, 기존 offline CLI parser 3개와 기존 offline planner/renderer/
CLI 회귀 33개, 기존 finite completion 2개가 통과했다. Rust 총 52개다.
browser model은 기존 항목 포함 44개, 기존 Worker/host 소비자는 522개가
통과했다. 첫 소비자 실행은
Node VM 모듈 flag가 빠져 시작하지 못했고, `--experimental-vm-modules`를
적용한 후 통과했다. 실패 원본도 보관했다. 첫 Rust 실행 도중 root의
format 정리가 있었으므로 최종 format 이후 위 50개를 반복하고 기존 finite
2개를 실행했다. 이는 독립 QA나 전체 완료 판정이 아니다.

로컬 원본 로그: `target/wf/realtime-replay-speed-development-12c90/`.
공통/native pause/CLI와 실제 browser 재생 연결 및 독립 review/QA는 남아 있다.

두 번째 묶음의 첫 실제 실행은 full-span pause fixture의 동일 source/host
관측을 NativePause가 거절했다. fixture를 진행된 두 clock 관측으로 바로잡았고,
권한 검증이나 기대값은 유지했다. 반복 실행에서 pause 13개가 통과했지만
StepReplay는 12개 중 10개가 실제 BgmFeeder의 SetRate 거절로 실패했다.
기존 feeder의 검증을 유지하는 명시적 immutable-rate opt-in을 AC011로
추가하고 StepReplay를 연결하는 중이다. 실패 로그는 그대로 보관한다.
이후 실제 feeder 8개와 기존 feeder 9개, pause 13개가 통과했고, StepReplay의
남은 한 fixture는 80-frame 요청이 자체 64-frame 한도를 넘었다. 실제
40+40-frame callback과 5초/10초 presentation으로 바로잡고 기존 기대값을
유지했다. 최종 재검사는 공유 재생 12개를 포함한 42개 모두 통과했다.
기존 step 19개, pause 4개, Stop feeder 3개도 통과했다. 앞선 실패 원본과
수정은 보관하며 source/test freeze 이후의 최종 재검사 결과를 사용한다.

현재까지 서로 다른 개발 테스트 범위는 Rust 120개, Node 566개다.
이는 독립 review/QA나 전체 player 완료를 의미하지 않는다. 다음 묶음은
native replay CLI 및 실제 BrowserReplay WASM 생성자 연결이다.

UI 변경 전 baseline screenshot은 위 로컬 증거 폴더의 `baseline-ui.png`에
보관했다. MCP 기본 브라우저는 COOP/COEP 격리를 확인했으나 WebGPU adapter가
없었고 favicon 404가 있었다. 이는 기존 UI layout 관측이며 재생 PASS가 아니다.
실제 재생 QA에는 기존 Puppeteer/Chromium 가상 GPU 경로와 새 WASM을 사용한다.
baseline 전용 localhost server는 종료했고 해당 탭은 about:blank로 복원했다.

Native 연결 개발 검사: 기존·신규 binary fixture 33개와 실제 CLI 4개가
통과했다. 별도로 opt-in ALSA null factory/recovery 진단 1개도 실제 실행해
통과했다. 이는 실제 native 소프트웨어 경로이며 물리·음향 검증은 아니다.
첫 native 컴파일은 direct `src/bin` fixture가 별도 binary로 발견되어
실패했다. fixture를 `src/bin/play_replay_bms/speed_fixtures.rs`로 옮기고
부모 cfg(test) 경로와 상대 setup 경로를 고쳤다. 테스트 기대값은 유지했다.

원래 workspace edition은 2021이다. root의 초기 scoped formatter 호출은
2024를 지정했으므로 Rust 파일을 2021 기준으로 다시 정리한 뒤 native
최종 검사를 실행했다. 현재 main/browser-audio WASM을 별도로 새로 생성하며,
실제 preparation-export 검사와 Worker/host/UI 연결은 남아 있다.

두 WASM 패키지를 각각 release 빌드하고 현재 bindgen을 생성했다. 실제
preparation-export 첫 실행은 11개 중 9개 통과, 2개 실패였다. 빈 녹화
로그에서는 기존 planner가 BGM을 제외하므로 무음과 빈 명령이 올바른
결과였다. 소리·기본 배속 비교 fixture에 실제 BrowserAudio 출력 관측으로
승인된 1µs Advance를 녹화하고, 기존 literal PCM·시간·종료·판정 기대값을
유지했다. 빈 로그 검사는 별도로 유지했다. 동일한 실제 WASM의 재검사는
11개 모두 통과했다. 제품의 기록 범위 정책은 바꾸지 않았다.
원본 실패와 재검사는 `preparation-wasm-1.log`, `preparation-wasm-2.log`에
보관한다. Worker 배속 연결과 독립 테스트 작성은 다음 구현 단계이며,
UI와 독립 review/QA, 필수 qa-browser PASS는 아직 완료하지 않았다.

현재 native replay binary의 Windows GNU·macOS 대상 Rust 타입 검사도
각각 exit 0으로 끝났다. 명령은 `target/toolchain/xcheck.sh windows|macos
-p beatkernel-bms-runtime --no-default-features --bin play_replay_bms`이며
로그는 `native-windows-1.log`, `native-macos-1.log`에 보관한다. 기존 app
library 경고 13개와 해당 binary의 `output_rate` 미사용 경고가 남아 있다.
이 스크립트는 C/C++ stub을 사용하므로 링크·장치 실행·물리 재생을 증명하지
않는다. native 개발 테스트 38개와 앞선 120개를 합한 Rust 검사 범위는
158개이며, 별도로 실제 WASM 11개가 통과했다.

Worker 배속 전달·실제 owner identity·finite endpoint 검증을 연결했다.
독립 작성한 9개 신규 검사와 기존 Worker 검사를 실행한
`node --experimental-vm-modules --test app/web/play-worker.test.mjs
app/web/worker.test.mjs`는 총 280개 통과, 실패 0이었다.
실제 새 WASM을 로드한 Worker 검사도 네 배속의 정규화한 owner·준비 응답과
원본 녹화 bytes·PCM sample export를 확인했다. UI 연결과 실제 브라우저의
Worker→Worklet 재생 검증은 여전히 남아 있으며 독립 QA PASS가 아니다.

UI 배속 필드·세션 snapshot·응답 대조를 연결한 뒤 실제 host 소스의
`node --experimental-vm-modules --test app/web/play-host.test.mjs` 검사는
277개 통과, 실패 0이었다. 신규 27개는 잘못된 배속의 오디오 시작 전 거절,
필드 포커스, 네 배속의 finite grid, immutable pending setup, 잘못된·누락된
owner identity, live 영향 없음과 stop/retry를 검사한다. 로그는
`replay-host-1.log`이다. 실제 브라우저 재생 runner는 작성 중이므로 AC008을
완료로 승격하지 않는다. 독립 review와 필수 qa-browser/qa-cli는 남아 있다.

추가 desktop·WebTransport all-targets 타입 검사의 첫 실행은 240초 제한으로
exit 124였다. 타입 오류는 확인되지 않았으나 통과로 취급하지 않는다.
로그 `desktop-consumers-1.log`를 보관하고, 해당 handle의 종료를 확인한 뒤
생성된 cache로 같은 검사를 다시 실행한다.
두 번째 동일 검사는 exit 0으로 끝났다(`desktop-consumers-2.log`). 기존
library·binary·example·fixture 경고는 남아 있으며 strict lint 통과를 주장하지
않는다. 모든 target의 타입 검사이며 전체 테스트 실행과는 구분한다.

첫 실제 Chromium 재생 probe는 graphics surface 준비 이전에 실패했다.
`target/wf/realtime-replay-speed-browser/development-1/`에 실패 screenshot과
source hash·미충족 요구를 보관했다. console/network 오류는 없었으나 UI는
스타일이 적용되지 않았다. 로컬 probe server의 CSS MIME 누락을 조사하며,
실제 브라우저 재생 성공이나 독립 QA PASS로 취급하지 않는다. 해당 probe의
owned Chromium과 server는 정상 cleanup 후 종료했다.
probe server의 CSS MIME를 `text/css`로 바로잡고 실제 draw receipt와 canvas
extent를 기다린 두 번째 실행에서는 실제 finite 2초 live capture가 통과했다.
녹화는 3443 bytes, Miss 2개, SHA256
`595642885619c4aed458f2db79d19ed98b04bbc5b204276a074a04c3e46baa79`다.
첫 1/2 리플레이의 progress 대조에서 -6535000ns 차이가 나 전체 실행은
exit 1이었다. 원본 raw timestamp와 실제 채택된 presentation point의
대조를 조사한다. Worker는 반복·회귀 estimate를 채택하지 않으므로 모든
render 응답이 새 point를 채택했다는 가정은 검증 근거가 아니다.
`development-2/evidence.json`에는 원본 메시지·출력 report·실패 화면을
보관한다. cleanup 중 audio WASM request의 ERR_ABORTED도 기록했으며,
실제 브라우저 전체 통과나 독립 QA PASS를 주장하지 않는다.
조사 결과 render121의 raw performanceTime 5137.655ms는 응답 관측
5136.665ms보다 미래였다. 기존 presentationPair가 이 estimate를 거절하여
render120의 곡 시간 284563090ns를 유지한 것이 확인됐다. fixture는 이미
초기화된 실제 BrowserReplay의 observe_output 인자와 결과를 관측해,
채택된 point 또는 동일 owner의 보존된 point에 원래 ±2ns 기대값을 적용한다.
반복·미래 estimate를 채택하도록 제품 정책을 바꾸지 않았고, 최소 두 개의
실제 progressing point 조건도 유지했다. 이 새 검사도 재실행 전에는
통과로 기록하지 않는다.
세 번째 실제 실행에서는 1/2 replay의 실제 accepted-point 시간 대조,
finite Worklet report와 PCM/pitch 검사가 통과했으나 visible progress 검사에서
exit 1이었다. probe가 재생 시간이 아닌 disabled preview seek draft 필드
`#position`을 읽은 것이 확인됐다. 실제 렌더러의 곡 시간·draw ACK·화면을
대조하도록 probe를 수정한다. 메인 스레드에 매 프레임 DOM 시간 갱신을
추가하지 않는다. 해당 실행의 console/network 오류는 모두 0이었다.
원본 `development-3/evidence.json`과 screenshot은 보관하며 모든 네 배속과
stop/retry·최종 독립 QA는 아직 완료되지 않았다.

네 번째 실제 Chromium 실행은 exit 0, 미충족 조건 0이었다. 현재 production
WASM→Worker→AudioWorklet 및 renderer에서 1/2·1·2·3/2 배속의 PCM/pitch,
엄격한 ±2ns 원본 곡 시간, 실제 finite consumed/applied frame marker,
draw ACK·재생 화면, natural completion, 사용자 stop/retry를 확인했다.
2초 원본의 44.1kHz 출력 endpoint는 각각 180810/92610/48510/63210 frame,
실제 progressing point는 각각 263/140/72/87개, mid-song draw는
150/75/38/49개였다. 원본 녹화 3358 bytes의 SHA256
`859082076d2ed97c7efb0131dcbfe476e755876fc73d59adbf11256a0e511728`가
유지됐고 console/page/network 오류는 0이었다.
`target/wf/realtime-replay-speed-browser/development-4/evidence.json`과
각 배속의 mid-play·종료 screenshot에 원본 관측과 source hash를 보관한다.
해당 owned browser/server는 종료했다. 이는 coordinator 개발 증거이며,
필수 독립 qa-browser·qa-cli와 code/security review PASS를 대체하지 않는다.

Known ceiling: 고정 양수 pitch-changing 리플레이 범위다. runtime/live/
retained/reverse와 물리 출력 검증은 원래 전체 Goal에 남는다. WBS는
91/193,47.15%,102 미완료이고05.13을 완료로 승격하지 않는다.
