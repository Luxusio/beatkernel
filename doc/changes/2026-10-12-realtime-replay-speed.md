# 실제 리플레이 owner의 고정 배속 연결

공통 StepReplay, native `play_replay_bms --speed NUM/DEN`, browser replay UI에
세션 시작 시 선택하는 고정 양수 유리수 배속을 연결했다. 동일한 배속이 실제
planner/Mixer read-head와 출력 presentation→원본 곡 시간 변환에 적용된다.
음높이도 바뀌며, 원본 녹화·판정·hash는 유지한다. 기본값은1/1이다.
요구 계약은 [실시간 배속](../kernel/REQ__realtime-replay-speed.md),
[리플레이 오디오](../kernel/REQ__bms-replay-audio.md)다.

구현 커밋은 `3b4f5ea`, 계약·개발 근거는 `da11de4`, converted pause 수정은
`fc2376a`다. 현재 `fc2376a`에 대해 독립 DEEP code/security review,
qa-cli, qa-browser와 scoped ux-browser의 실제 final이 모두 PASS다.
최종 문서 critic과 Harness verify/close는 이 문서 작성 시 아직 실행 전이며
그 결과를 위 PASS와 혼동하지 않는다. 전체 player Goal은 미완료다.

## 구현과 정수 경계 수정

ReplaySpeed는 checked i128로 `start + trunc((elapsed-preroll)*N/D)`를
계산한다. 검증된 finite end만 narrowing 전에 cap할 수 있다. Preroll과
승인된 pause gap은 출력 시간이다. 기존 constructor는 NORMAL을 유지하고,
명시적인 immutable-rate BgmFeeder 경로만 SetRate를 허용한다. 원래 command
순서·Stop credit·producer/ACK retirement를 재사용한다.

Quantized/zero output fence에서도 정확한 consumed/applied와 feeder retirement,
실제 source presentation 또는 original converted target crossing이 확보됐을
때만 남은 원본 record를 validated end까지 진행한다. 새 clock/owner/가짜
records_finished나 host-clock 대체는 없다.

첫 독립 code review는 converted NativePause가 gap을 ZERO 기준으로 먼저
좁히는 결함을 발견해 FAIL했다. sibling presentation-origin에도 같은 문제가
있었다. 독립 작성한 세 회귀가 기존 소스에서 실제 실패한 뒤 `fc2376a`에서
MIN/MAX anchor를 이용해 duration과 startup shift를 wide로 합치고 마지막
값만 좁히도록 수정했다. 기존 TargetTime의 단일 floor와 domain/startup/order
검증은 유지한다. 실제 수세기 PCM을 실행했다는 뜻은 아니다.

## 독립 실행 근거

CLI QA 원본은 `target/wf/qa-cli-realtime-12c90-1/`이다.

- 신규 Rust6 suite54 PASS, 기존 pause100 PASS, native binary33 PASS.
  ignored null factory diagnostic1개도 명시적으로 실행해 PASS했다.
- 기존 offline audio/planner/renderer/actual CLI33, parser3, step19,
  finite completion2, strict feeder3, output Stop3와 추가 BGM26 PASS.
  필터가 겹치므로 독립 테스트 총수로 더하지 않는다.
- Node model44/Worker280/host277/실제 current WASM11: 총612 PASS.
- 실제 native CLI help·invalid·reduced·max 및 네 배속 null 출력은 총9경로로
  확인했다. 정상 exit0, zero/duplicate exit1,6/4→3/2 정규화 및 동일 원본
  judge hash `0x7d4034e9845b3301`, normal1/nonnormal2 command consumed/applied,
  xruns/suspends/failures0과 complete converter recovery를 확인했다.
  `--seconds`는 wall cutoff이고 null은 presentation이 없으므로 자연종료
  증거로 사용하지 않는다. QA가 처음 `--seconds 0.05`를 잘못 입력한 거절
  원본도 보존했다. 제품의 정수 seconds 계약은 바꾸지 않았다.

Browser QA는 actual production UI→Worker/WASM→AudioWorklet/WASM→renderer를
직접 실행한 `interactive-browser`다. 최종 증거와 AC별 transcript:
`target/wf/realtime-replay-speed-browser/qa-browser-2/`.
1/2·1·2·3/2에서 실제 PCM/pitch, ±2ns 원본 곡 시간, genuine finite marker,
자연종료, mid-song draw, Stop/retry와 원본 record identity를 확인했다.
44.1kHz endpoint는180810/92610/48510/63210 frame이다. 실제 녹화3596 bytes,
Miss2, SHA256 `5ea80dccddebbb3a566909be1b24633ba34627aeb59b21a9d7d7ca0a82219cbb`는
유지됐다. 필수8그룹 PASS, console/page/network0, unmet[]다.

첫 독립 browser 실행은 기능7그룹을 통과했지만 initialization audio WASM의
`ERR_ABORTED`1건으로 exit1이었다. 같은 source/package SHA와 strict oracle로
한 번만 반복했을 때 재현되지 않아 transience로 기록한다. 원본
`qa-browser-1/`은 보존했고 원인·영구 해결을 추정하거나 network 검사를
완화하지 않았다. 반복 재현 시 initiator/timing 조사가 필요하다.

독립 UX 증거는 `target/wf/ux-browser-realtime-12c90-1/run-1/`이다.
잘못된 draft5개는 오디오 생성 없이 focus/editability를 유지했고,
0006/0004→3/2 키보드 시작·Space Stop과390/768/1200px 배치를 확인했다.
실제 네 배속 재생도 PASS, console/network0이다. 상세 오류가 focused field
viewport 밖에 있을 수 있어 inline error는 비차단 polish 후속 항목이다.
모든 owned Chromium/server와 Cargo 실행 핸들은 종료됐다.

수정 후 coordinator도 Windows/macOS native binary typing, desktop/WebTransport
check, 두 WASM release build/bindgen과 actual exports11 PASS를 확인했다.
로그는 `target/wf/realtime-replay-speed-development-12c90/`의
`native-windows-2.log`, `native-macos-2.log`, `desktop-consumers-3.log`,
`browser-wasm-build-2.log`, `audio-wasm-build-2.log`, `preparation-wasm-3.log`다.
이것은 독립 QA와 별개이며 foreign C/C++ stub은 Rust typing만 증명한다.
기존 compiler 경고가 남아 있어 strict lint 완료를 주장하지 않는다.

## 보존한 실패와 재현 경로

개발 첫 실행들의 VM flag 누락, Cargo autobin fixture 배치, empty-record PCM
가정, callback budget 초과, formatter edition 지정,240초 check timeout은
원본 로그와 함께 보존했다. 실제 browser development-1/2/3은 probe server
CSS MIME, 거절된 future association, disabled preview seek draft를 progress로
읽은 검사 오류로 실패했다. probe를 실제 CSS MIME·채택된 point/보존된 point·
renderer draw ACK로 수정했고 제품 권한/기대값은 완화하지 않았다.
development-4 actual PASS는 개발 증거이며 최종 독립 QA를 대신하지 않는다.

실제 browser 재현 명령은 기존 Puppeteer-core/Chromium을 사용한다:

```sh
PUPPETEER_MODULE=target/wf/record-score-ui-qa-browser/node_modules/puppeteer-core \
CHROMIUM=/usr/bin/chromium \
REPLAY_SPEED_OUT=target/wf/realtime-replay-speed-browser/fresh-run \
REPLAY_SPEED_PORT=8167 \
timeout --signal=TERM --kill-after=10 240 node app/web/replay-speed.browser.mjs
```

Fresh ignored output와 비어 있는 port를 사용한다. Runner는 소유한 HTTPS
server/Chromium만 정리하며 strict console/network 실패를 그대로 보고한다.

## Known ceiling

Known ceiling: 고정 양수 pitch-changing 리플레이 범위다. runtime/live/
retained/reverse와 물리 출력 검증은 원래 전체 Goal에 남는다. WBS는
91/193,47.15%,102 미완료이고05.13을 완료로 승격하지 않는다.
Known ceiling: ALSA null은 실제 native 소프트웨어 startup/명령/회수만
확인한다. presentation 없는 wall cutoff 실행은 자연종료·음향 alignment
증거가 아니다. foreign typing은 실제 Windows/macOS SDK 링크·장치 실행이 아니다.
Known ceiling: 실제 Chromium의 소프트웨어 GPU/audio 실행은 물리 장치의
음향·입력 지연이나 세계 최고 성능을 증명하지 않는다.
