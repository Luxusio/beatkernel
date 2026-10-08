# BMS player 구현/검증 현황 — 2026-10-07

현재 전체 TODO/진행률은 [전체 WBS](REQ__bms-player-work-breakdown.md)를 사용한다.
`python3 tools/wbs_status.py`가 말단 작업의 완료·검증 대기·구현 중·장비/정책 대기를
집계한다. 아래 60개 행은 stable 기능 ID와 과거 source survey이며 완료 분모가 아니다.

2026-10-08 재개 주의: 아래는 이전 revision의 기능 인벤토리이며 현재
완료율/TODO 집계가 아니다. 전용 native input collector는 새 구현과
독립 검증을 마쳤다(REQ__native-input-collector.md, closed task). 실제
same-rate ALSA/app pending PCM 소유권도 REQ__native-output-continuity.md의
새 검증을 마쳤다. 별도 browser renderer Worker·live class/EX·mines·records
경로 역시 최신 source/evidence를 우선한다. 전체 행의 재조사·독립 사실
검토는 TASK__parallel-player-requirements의 범위 ledger에서 진행한다.
아래 과거 상태나 수를 근거로 이 기능들을 중복 구현하지 않는다.

조사 상태: 독립 사실/범위 리뷰 대기. 이전 슬롯 제한은 재시작 후 해제됐지만
이 인벤토리 자체의 독립 리뷰는 아직 완료되지 않았다. 아래 숫자는 source 조사 집계이며,
독립 검토된 기능 완료 개수로 인용하면 안 된다.

기존 조사 기준 source revision: `5ddc850`. BK-030은 `50d088d` 이후 현재 작업 트리의 연동 상태를 반영한다. 아래는 현재 알려진 요청/기능을 묶은 **기능 단위 1차 인벤토리**다. 전체 목표는 `bms player개발해`이며 [원래 계획](../../plan.md), [BMS player 요구사항](REQ__bms-player.md), native/browser/competition/gauge/adapter 계약의 모든 세부 조건을 유지한다. 이 표가 그 조건을 삭제하거나 새로운 축소된 성공 기준을 만들지 않는다. 각 기능 내부의 세부 TODO를 전부 원자화한 목록은 아직 아니다. 향후 세부 audit 또는 새 요구사항이 생기면 stable ID와 근거를 추가한다.

## 집계의 의미

- S: 관찰한 주요 구현 경로가 존재한다. **해당 기능의 모든 세부 조건 충족/출시 완료라는 뜻이 아니다.**
- P: 코드 일부가 있으나 명시적인 구현/연동/검증 작업이 남아 있다.
- N: 필요한 구현 경로를 찾지 못했고 현재 조사 범위에서 미구현으로 분류한다. 다른 경로가 확인되면 근거와 함께 수정한다.
- U: 현재 조사로 구현 여부를 결정하지 못했다. 존재하는 주변 코드로 완료를 추정하지 않는다.
- C: 원래 계획의 조건부 요구이며 현재 미활성이다.

검증 열: L은 확인한 순수/로컬 fixture의 current suite 통과, D는 직접 관찰한 GUI/browser 증거(독립 QA 완료와 다름), H는 실제 native/hardware 증거 필요, Q는 추가 계약/연동/독립 검증이 필요함을 뜻한다. L/D도 그 행 전체의 최종 완료 판정이 아니다.

**총 60개 기능 항목: 활성 59개, 조건부 1개.**
활성 상태 집계: S 39, P 18, N 1, U 1.
이는 source survey이며, S를 완료 개수로 바꾸거나 P를 임의로 50% 가중해 전체 구현률을 계산하지 않는다. **최종 완료 개수/전체 완료율은 아직 증명되지 않았다.**

이전 `5ddc850`/retained-ui-motion 조사에서 Rust workspace/all-targets 결과는 2,724 passed / 0 failed / 6 ignored였다. 기존 hardware/relay ignored 사례는 통과로 계산하지 않았다. 당시 명시 desktop,webtransport runtime lib/bins는 2,028 passed / 0 failed / 2 ignored였다. 해당 이전 실행은 [로그](../../target/wf/ui-motion-qa-cli/workspace-tests.log)에 있다. 새 live-class 작업의 Runtime/Player 10개와 HUD 3개 scoped fixture도 통과했다. 이전 결과를 새 소스의 전체 workspace 통과로 인용하거나 다른 언어·물리 장치·전체 Goal acceptance까지 확장하지 않는다.

현재 `394c2c0` audio-authority 연동에서는 명시 desktop,webtransport app lib/bins가 2,175 passed / 0 failed / 2 ignored다([로그](../../target/wf/audio-authority/runtime-launcher-regression.log)). Legacy capture/playback/native-feed integration 21개와 Node browser 524개가 통과했고 Windows GNU/macOS all-targets C/archive-stub 및 browser WASM 검사도 통과했다. 이들은 개발 회귀·source/type 증거다. 새 full workspace 및 formal review/독립 CLI·desktop·browser QA는 아직 완료되지 않았고 실제 SDK/hardware·음향 정확도/성능을 증명하지 않는다. 다음 행의 상태 집계나 전체 Goal 완료 판정은 바꾸지 않는다.

## 기능별 source / 남은 작업

| ID | 영역 | 요청/기능 | 구현 조사 | 검증 | 실제 근거와 남은 범위 |
|---|---|---|---|---|---|
| BK-001 | 코어 | 정수 시간·오버플로 경계 | S | L | [경로](../../crates/beatkernel/src/time/mod.rs) — checked 시간 타입과 시간/transport fixture 존재; 물리 동기화와는 별도 |
| BK-002 | 코어 | rate/pause/seek/reverse transport | S | L | [경로](../../crates/beatkernel/src/transport/mod.rs) — 공통 논리 transport; 네이티브 음향 동기화는 별도 검증 |
| BK-003 | 코어 | 정규 입력·장치/컨트롤 식별 | S | L | [경로](../../crates/beatkernel/src/input/mod.rs) — 타입·codec·provenance/입력 fixture; 실제 장치는 H 행에서 확인 |
| BK-004 | 코어 | 장치별 binding·fanout | S | L | [경로](../../crates/beatkernel/src/input/binding.rs) — exact/any-device binding; 자동 장치 선택과 로컬 roster는 별도 |
| BK-005 | 코어 | 채보 compiler·source identity | S | L | [경로](../../crates/beatkernel/src/chart/compiler.rs) — 컴파일·범위·golden fixture; 모든 BMS 방언 준수는 별도 |
| BK-006 | 코어 | Instant/Hold/Tracking/Repeated/Composite judge | S | L | [경로](../../crates/beatkernel/src/judge/engine.rs) — 판정/소유권/일반화 fixture; BMS 표준 timing preset 완성은 별도 |
| BK-007 | 코어 | 통합 Runtime·local RuntimeGroup | S | L | [경로](../../crates/beatkernel/src/runtime/mod.rs) — 공통 judge/input/audio 교환과 local-group fixture |
| BK-008 | 코어 | 결정론 replay/snapshot/restore | S | L | [경로](../../crates/beatkernel/src/replay/mod.rs) — 실시간과 동일 judge 전이; 음향 재시작 보증은 별도 |
| BK-009 | 코어 | Mixer·PCM·예약 명령/키음 | S | L | [경로](../../crates/beatkernel/src/audio/mixer.rs) — Mixer/queue/PCM fixture 및 offline render; 물리 출력은 별도 |
| BK-010 | 코어 | 순수 FormatConverter·channel matrix | S | L | [경로](../../crates/beatkernel/src/audio/convert.rs) — rate conversion 알고리즘 존재; native channel_remix는 같은 rate만 허용 |
| BK-011 | 코어 | telemetry·benchmark 도구 | S | Q | [경로](../../crates/beatkernel/src/telemetry/mod.rs) — 계측 코드 존재; 실제 p99/jitter/latency/underrun 결과 필요 |
| BK-012 | Native IO | Windows Raw Input·HID | S | H | [경로](../../crates/beatkernel-platform/src/windows/input.rs) — 구현 경로 존재; 현재 Windows 실제 입력/대응 장치 실행 필요 |
| BK-013 | Native IO | Windows WASAPI shared/exclusive | S | H | [경로](../../crates/beatkernel-platform/src/windows/audio.rs) — 스트림/관측/제어 코드; 실제 드라이버·소리/지연 검증 필요 |
| BK-014 | Native IO | 선택형 ASIO backend | S | H | [경로](../../crates/beatkernel-platform/src/windows/asio/stream.rs) — 외부 SDK/C++ bridge source; 실제 MSVC/SDK/driver 실행 미확인 |
| BK-015 | Native IO | Linux evdev/hidraw/ALSA | S | H | [경로](../../crates/beatkernel-platform/src/linux/alsa.rs) — 네이티브 구현; ALSA null이 물리 presentation 증거는 아님 |
| BK-016 | Native IO | macOS IOHID/CoreAudio | S | H | [경로](../../crates/beatkernel-platform/src/macos/audio.rs) — 구현 경로/타입 검사와 실제 장치 실행 구분 |
| BK-017 | Native IO | buffer/channel/rate 설정·live output 제어 | P | Q | [경로](../../app/src/gameplay/output/domain/control.rs) — 포트/요청/교체·같은 rate 채널 정책 구현; 전체 native 조합/수명 검증 필요 |
| BK-018 | Native IO | 독립 키 입력 수집 스레드 | N | Q | [경로](../../app/src/native_gameplay_host.rs) — 전용 입력 collector를 찾지 못함; native/game owner 수집과 UI/audio 분리만으로 충족되지 않음 |
| BK-019 | Native IO | native rate 변환 phase/history/frontier 연결 | P | Q | [경로](../../crates/beatkernel-platform/src/audio/channel_remix.rs) — source.sample_rate != target.sample_rate를 거부; 순수 converter 존재만으로 연결 완료 아님 |
| BK-020 | Native IO | WASAPI↔ASIO 등 cross-backend live 교체 | U | Q | [경로](../../app/src/gameplay/output/application/replacement.rs) — 교체 상태 머신 존재; 서로 다른 backend 전환 전체 계약은 이번 조사에서 증명하지 못함 |
| BK-021 | Native IO | audio master clock·변경된 latency 대응 | P | H | [경로](../../app/src/native_audio_presentation.rs) — native/browser raw OUTPUT scheduling·logical OUTPUT Runtime/Transport 및 unknown HOST input correlation 연동; 전 backend/rate/buffer 교체 및 음향 증거 필요 |
| BK-022 | BMS | 별도 BMS adapter/runtime 경계·parser | S | L | [경로](../../adapters/beatkernel-bms/src/lib.rs) — adapter/core/app crate 경계; parser fixture와 특수 방언 준수 구분 |
| BK-023 | BMS | BPM/STOP/분기/long-note/mine 처리 | P | Q | [경로](../../adapters/beatkernel-bms/src/lib.rs) — 여러 처리 경로와 fixture 존재; 모든 legacy dialect/판정 호환성 완성은 아님 |
| BK-024 | BMS | WAV/FLAC/Vorbis/MP3 asset decode | S | L | [경로](../../app/src/lib.rs) — DefaultAssetDecoder의 flac_decode/vorbis_decode/mp3_decode 경로 구현; chained Ogg 등 전 포맷 준수는 아님 |
| BK-025 | BMS | static BGA·opacity/crop/canvas | S | L | [경로](../../app/src/image_assets.rs) — BMP/PNG/JPEG·layer/crop/opacity 경로; video/full ARGB는 별도 |
| BK-026 | BMS | BGA video·EXBMP RGB/color-key 완전 지원 | P | Q | [경로](../../doc/kernel/REQ__bms-adapter.md) — 정적 이미지와 일부 EXBMP metadata 존재; video/ARGB RGB/color-key 명시적으로 미완성 |
| BK-027 | BMS | native 라이브 플레이·finite completion | S | H | [경로](../../app/src/native_gameplay_host.rs) — native pump/완료·custody 코드; 실제 입력/오디오 전체 실행 필요 |
| BK-028 | BMS | gauge·판정 class·EX 계산 기반 | S | L | [경로](../../app/src/judgment_policy.rs) — 명시 class mapping·gauge·EX projection 구현; opaque grade로 class 추정하지 않음 |
| BK-029 | BMS | 완전한 표준 BMS timing windows/empty POOR 호환 | P | Q | [경로](../../app/src/bin/linux_bms.rs) — native 도움말은 한 PGREAT window와 POOR misses, full LR2 windows 아님을 명시 |
| BK-030 | BMS | 실시간 member별 class/EX snapshot·HUD | P | Q | [경로](../../app/src/player.rs) — 선택 정책 cold admission과 member별 scalar class/EX snapshot·HUD, 공통 native 호출부 작성됨; 새 fixture·독립 review/QA 검증 진행 중 |
| BK-031 | 기록/연습 | accepted-operation capture·logical replay | S | L | [경로](../../app/src/replay_capture.rs) — 원래 선택/operation과 policy metadata 보존; 실제 장치 지연 역재현은 아님 |
| BK-032 | 기록/연습 | 기록 catalog/archive·과거 등급/EX 상세 | S | D | [경로](../../app/src/record_catalog.rs) — prefix와 stored score 분리 구현; local fixture/관찰 증거, broad Goal 완료 아님 |
| BK-033 | 기록/연습 | Watch replay·native audio drain | S | H | [경로](../../app/src/replay_audio.rs) — 공유 Renderer/Runtime와 출력 playback; backend별 실제 terminal presentation 검증 필요 |
| BK-034 | 기록/연습 | 특정 구간 시작·논리 seek/restart | S | L | [경로](../../app/src/section_start.rs) — 원래 song 범위·입력/키음 policy 유지; 실제 음향 alignment는 별도 |
| BK-035 | 기록/연습 | gapless loop/scrub·음향 sync 보증 | P | H | [경로](../../app/src/practice_loop.rs) — practice/retry/loop 구조 존재; reopen gap·sample-exact seamless/acoustic 검증 미완성 |
| BK-036 | 로컬/경쟁 | single-player 기본 장치 선택 | S | H | [경로](../../app/src/desktop.rs) — 일반 UI에서 device 선택 강제하지 않는 경로; OS별 실제 자동 장치 확인 필요 |
| BK-037 | 로컬/경쟁 | 2..64 roster·장치 할당·같은 출력 | S | H | [경로](../../app/src/local_players.rs) — original member IDs/장치·공유 출력 구조; 실제 여러 장치 실행 필요 |
| BK-038 | 로컬/경쟁 | 자기 과거 기록과 경쟁 | S | L | [경로](../../app/src/saved_opponents.rs) — 실제 기록 prefix 재구성/적합성 검사 |
| BK-039 | 로컬/경쟁 | 타인 저장 기록과 경쟁 | S | L | [경로](../../app/src/saved_opponents.rs) — 동일 기록 engine, own/other 표시는 인증된 identity 아님 |
| BK-040 | 로컬/경쟁 | 공통 QUIC transport | S | Q | [경로](../../app/src/multiplayer_quic.rs) — transport/프로토콜 코드와 fixture; 실제 플랫폼 상호운용 범위 확인 필요 |
| BK-041 | 로컬/경쟁 | WebTransport transport | S | Q | [경로](../../app/src/multiplayer_webtransport.rs) — Rust 서버/client/JS bridge; current scoped browser rendering은 network 검증 아님 |
| BK-042 | 로컬/경쟁 | room/start/progress/results·local cohort 연결 | P | Q | [경로](../../app/src/multiplayer_room_start.rs) — room/통합 경로 다수 존재; 실제 N-member native/browser 전체 실행/정책 조합 검증 필요 |
| BK-043 | 로컬/경쟁 | custom class/gauge policy의 network 전면 연동 | P | Q | [경로](../../app/src/native_policy_admission.rs) — 선택형 local/replay 기반 존재; native 도움말은 nondefault multiplayer 제한 명시 |
| BK-044 | UI | 단일 app·winit/wgpu 렌더러 | S | D | [경로](../../app/src/graphics.rs) — current native Renderer와 browser Worker 실제 관찰; hardware/perf 보증 아님 |
| BK-045 | UI | screen/fragment 수명·back stack·취소/cleanup | S | L | [경로](../../app/src/screen_lifecycle.rs) — Navigator/owner lifecycle 구현과 fixture; 모든 화면 조합 검증은 남음 |
| BK-046 | UI | typed 선언형/retained/무 Virtual DOM 기본층 | S | D | [경로](../../app/src/ui/layout.rs) — Display 첫 migration·mount-only resolve·부분 packet 갱신 |
| BK-047 | UI | 모든 화면 선언형 이전·동적 layout invalidation | P | Q | [경로](../../app/src/ui/display.rs) — 현재 하나의 완전한 static 화면; 다른 화면/resize reflow/parent-child 갱신 필요 |
| BK-048 | UI | 개별 component transform·animation scheduling | P | Q | [경로](../../app/src/ui/motion.rs) — scene 전체 integer translation/pure sampler만 구현; per-node/fraction/easing/scheduler 없음 |
| BK-049 | UI | font/IME/clipboard/text input | S | L | [경로](../../app/src/ui/text_input.rs) — 편집/IME/clipboard 경로·fixture; native 입력 방법별 실제 확인 별도 |
| BK-050 | Browser | WASM·OffscreenCanvas Worker 렌더링 | S | D | [경로](../../app/web/worker.js) — current 실제 WASM/Worker shader default-offset 관찰; nonzero browser motion 아님 |
| BK-051 | Browser | AudioWorklet·오디오 시계/playback | S | Q | [경로](../../app/web/audio-worklet.js) — source/JS fixture; 실제 browser audio/latency/end-to-end 검증 필요 |
| BK-052 | Browser | keyboard/touch/HID·local players | S | Q | [경로](../../app/src/browser_input.rs) — 각 입력 owner와 fixture 존재; 실제 장치/브라우저 제한/표현력 검증 필요 |
| BK-053 | Browser | Window는 입력 중심·모든 UI/render worker화 | P | Q | [경로](../../app/web/main.js) — Canvas worker화 됐지만 Window DOM controls/status와 browser/native feature parity 남음 |
| BK-054 | 구조/출시 | DDD·UI/business/native IO 포트/DI 전면 분리 | P | Q | [경로](../../doc/kernel/ADR__application-domain-modules.md) — gameplay/output부터 이전; 모든 context/layer 경계의 완전한 분리 아님 |
| BK-055 | 구조/출시 | zero-cost hot path·세계 최고 성능 입증 | P | H | [경로](../../doc/kernel/REQ__runtime-benchmark.md) — 설계 목표/계측 도구; 비용/alloc/lock audit과 비교 workload·실측 결과 필요 |
| BK-056 | 구조/출시 | SQLite급 계층/결정론/fuzz/stress 안정성 | P | Q | [경로](../../doc/kernel/REQ__plan-acceptance-evidence.md) — 2,724 current Rust tests는 유한 corpus, 지속 fuzz/chaos/경계 완전성 보증 아님 |
| BK-057 | 구조/출시 | Windows+다른 OS 실제 input/audio/latency 매트릭스 | P | H | [경로](../../doc/kernel/REQ__plan-acceptance-evidence.md) — Rust/stub cross-check와 물리 재생/입력/1kHz/jitter/underrun 증거는 다름 |
| BK-058 | 구조/출시 | MIT 소스·ASIO 제외 MIT / 포함 GPLv3 배포 | S | Q | [경로](../../doc/platform/REQ__asio-distribution.md) — 승인된 source/build 정책; 실제 combined release artifact/SDK 배포 감사 별도 |
| BK-059 | 구조/출시 | 최종 docs/examples/build/release/독립 QA | P | Q | [경로](../../doc/kernel/REQ__implementation-status.md) — 현재 source/tests 다수 존재; two UI QA lenses blocked, 부모 scope/실제 release proof 미완료 |
| BK-060 | 조건부 | C ABI/C# SDK | C | Q | [경로](../../doc/kernel/REQ__sdk-status.md) — 확인된 host 요구 없어서 미활성; active TODO 분모에서 제외, 삭제/완료 처리 아님 |

## 조사 근거와 최신성

Native rate 변환 연결은 `audio/channel_remix.rs::validate`가 rate 불일치를 거부하는 실제 분기로 확인했다. 기존 조사에서 확인한 live class HUD 부재는 현재 작업 트리에서 `prepare_play_policies`, scalar `bms_score`, policy-aware 공통 native bridge 및 HUD 연동으로 보완됐다. 현재 독립 검증이 끝나지 않아 BK-030은 P/Q를 유지한다. 별도 input collector는 관련 native/플랫폼 input 코드의 owner/thread 경로에서 찾지 못했다. Video/EXBMP RGB/color-key는 adapter 계약에 미완성으로 명시돼 있다. 개별 UI transform/layout는 현재 whole-scene uniform과 mount-only Node resolver 범위를 대조했다.

Source module/manifest/symbol inspection은 소스 존재 증거다. 모든 구현의 correctness를 증명한 audit가 아니다. 위 broad 기능과 underlying 세부 REQ 사이의 미검증 조건을 없애지 않는다. 기존 parent PLAN의 AC reference/제목 숫자와 이 기능 단위 행은 서로 다른 분모이므로 합산하지 않는다. 이 문서는 제품 source 현황이며 Harness acceptance ledger가 아니다; task PLAN과 ordered review/QA가 해당 task의 완료 권한이다.

## 확인된 우선 구현 / 검증 항목

1. BK-030: 작성된 live class/EX 경로의 Runtime/Player/HUD 및 native/WASM 회귀 검증과 독립 review/QA를 완료한다. recorded/historical score와 live score를 혼동하지 않게 한다.
2. BK-018: source clock/provenance를 유지하는 bounded 입력 collector와 게임 처리 owner 분리; overflow/late/cancel/join 순수 fixture 및 실제 native 검증 준비.
3. BK-019/BK-020/BK-021: rate conversion과 backend 교체에 대한 source/output frame·history/phase·clock/latency lifetime을 연결하고 검증.
4. BK-047/BK-048: retained layout invalidation과 per-component transform을 실제 UI 입력/클리핑과 함께 추가. Virtual DOM은 도입하지 않는다.
5. BK-029/BK-043/BK-053: 표준 BMS 등급 timing/empty POOR, custom policy network/브라우저 경로, worker 중심 UI 전면 이전.
6. BK-055/BK-056/BK-057/BK-059: 측정/정적·동적 RT audit/지속 stress/fuzz/실제 여러 OS input/audio 및 최종 독립 acceptance.

현재 UI child의 code/security/CLI는 recorded PASS이며 desktop/browser 독립 QA는 host agent 슬롯 제한으로 미완료다. coordinator의 실제 GPU 이동·click/resize 및 default-offset browser Worker 관찰은 [desktop 증거](../../target/wf/ui-motion-qa-desktop/transcript.md), [browser 증거](../../target/wf/ui-motion-qa-browser/transcript.md)에 있다. 이는 독립 QA 또는 물리 latency/performance 완료가 아니다. source 코드/검증 artifact가 변하면 각 행의 근거 범위를 다시 확인한다.
