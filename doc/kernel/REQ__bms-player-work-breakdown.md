# BMS 플레이어 전체 작업 분해 구조(WBS)

기준일: 2026-10-14. 목표: **범용 BeatKernel 기반의 고성능 cross-platform BMS 플레이어**.
사용자가 요청한 전체 TODO/진행률의 기준 문서다. 현재 병렬 개발 묶음만으로
전체 목표를 대체하지 않는다. [원래 명세](../../plan.md),
[플레이어 요구사항](REQ__bms-player.md),
[전체 완료 조건](REQ__plan-acceptance-evidence.md),
[기능 ID 인벤토리](REQ__bms-player-progress.md)의 활성 요구를 유지한다.
원래 명세의 초기 비목표는 이후 사용자가 승인한 UI/browser/멀티플레이 요구를
제외하는 근거가 아니다. 검증 연기는 2026-10-06 해제됐다.

## 읽는 법과 집계

- **말단 체크박스만 센다.** 하위 항목이 있는 부모는 구조/원래 요구 범위를 유지하며
  말단 분모에 중복 합산하지 않는다. `BK-WBS-07.08.01`처럼 기존 ID 아래에 안정적인
  하위 ID를 붙인다. 원래 상위 제품 요구사항 집계와 확장된 실행 단위 집계를 함께 표시한다.
- `[x]` + `상태=D`: 해당 문장의 **범위에 한정된 구현과 실제 검사 근거**가 있다.
  순수 helper 완료가 실제 launcher, GUI, hardware 완료를 의미하지 않는다.
- `[ ]` + V: 코드가 있지만 이 문장에 필요한 current 실행/연동/독립 확인이 남았다.
  W: 구현·연동 중, N: 미구현, E: 외부 환경/장비 필요, P: 정책 선택 필요,
  U: 최신 현황 감사 필요. E/P도 활성 분모에 남고 완료로 계산하지 않는다.
- C: 명시적인 조건부·미활성 범위다. 현재 분모에서 제외하며 삭제하지 않는다.
- 진행률은 **D / 활성 말단 개수**다. 동일 가중치의 체크리스트 비율이며
  소요 시간/공수/세계 최고 성능/출시 준비도를 추정하는 백분율이 아니다.
  구현됨 항목에 임의의 50% 가중치를 주지 않는다.
- 전체 제품 완료는 마지막 release/원본 §24 audit까지 모두 충족해야 한다.
  source survey의 S, 현재 task AC 완료 수, test PASS 개수로 대체하지 않는다.
- ID는 유지한다. 완료 범위를 더 쪼개거나 새 요구가 확인되면 새 ID를 추가하고
  아래 변경 이력에 분모 변경을 적는다. 실패·source 변경으로 증거가 무효가 되면
  해당 leaf를 V/W로 되돌리고 다음 검사를 적는다.
- 진행 중인 구현 작업은 실제 구현·연동·검증 경계에 맞춘 하위 실행 단위로 연결한다.
  각 단위에 상태, 범위에 한정된 근거와 다음 검사를 기록하고 구현/검증 milestone이
  끝날 때 갱신한다. 원래 요구 범위·환경/정책 대기를 쪼개는 과정에서 없애지 않는다.
- 부모의 완료는 모든 활성 말단과 원래 범위 감사가 일치해야 한다. 미완료 말단이
  있는 D 부모, 모든 말단이 D인데 미완료로 남은 부모, 활성 말단이 없는 활성 부모는
  오류다. 부모의 선행 조건은 자식에게도 적용하고 C 부모 아래 활성 자식은 허용하지 않는다.

집계/ID/상태/선행/기능 coverage 확인:

```sh
python3 tools/wbs_status.py
python3 tools/wbs_status.py --json
python3 tools/wbs_status.py --ready
python3 tools/wbs_status.py --in-progress
```

숫자는 위 명령이 본문에서 계산한다. 별도 수동 합계표를 동기화할 필요가 없다.
기본/JSON 보고의 활성·완료·남은 수는 확장된 말단 기준이며 원래 상위 요구사항
집계를 함께 표시한다. 원래 활성193개·D91개·조건부4개는 이번 분해로 바뀌지 않는다.
`구현됨` D+V는 코드가 있는 단위 수이며 완료 수 D와 다르다. `--ready`는 부모의
원래 범위와 상속된 선행 조건까지 확인하고, `--in-progress`는 말단 W/V/N을
보여 준다. E/P/U/C는 본문과 상태 집계에 계속 남는다.
JSON 계약: 기존 `active`, `done`, `remaining`, `percent`, `conditional`,
`states`, `groups`는 말단 작업을 집계한다.
`requirements.{active,done,remaining,percent,conditional}`는 원래 상위 요구사항
집계이며, `parents`는 구조 부모 수, `implemented`는 말단 D+V 수다.
`mapped_features`는 기능 인벤토리 coverage 수이며 작업 완료 수가 아니다.
상태 줄의 `선행=-`는 이 문서에 추가로 명시한 실행 선행이 없다는 의미이며,
아래 실제 개발 의존성/공통 파일 장벽까지 없다는 의미가 아니다.

## 근거 인덱스

근거는 각 행의 날짜와 checkout에서 확인한 개발/QA 기록이다. 로그는 ignored
`target/`에 있으므로 새 clone에서 사라질 수 있다. 테스트 source/계약과 재현
명령도 함께 남긴다. 로그 유실을 PASS로 대체하지 말고 필요한 scope를 재실행한다.

| 코드 | 확인한 실제 근거 | 범위/한계 |
| --- | --- | --- |
| E0 | 닫힌 native-output-continuity child의 실제 workspace QA: `target/wf/native-output-continuity/qa-cli-2/workspace.log`; core time/transport/input/binding/chart/judge/replay/runtime/visual 경로에 현재 diff 없음 | 변경 없는 해당 core/fixture 범위에만 사용. 당시 workspace 3072 + doctest20 PASS, 실패0, ignored6; 새 전체 source QA가 아님 |
| E1 | `target/wf/parallel-player-requirements/combined-app-development.log`: app lib `desktop,webtransport` 2011 PASS / 실패0 / ignored3 | 해당 시점의 pure/app/retained/helper fixture. actual current WASM/GUI/native hardware/full workspace를 증명하지 않음 |
| E2 | `resume-core-development.log`: 실제 converted boundary/mixer 23 PASS; 앞선 target-time 31 PASS | exact duration/phase/boundary 및 cold refusal |
| E3 | `resume-platform-development.log`: platform 126 PASS / 실패0 / ignored1; 후속 `provenance-platform-development.log`: 128 PASS / 실패0 / ignored1 | 실제 생산 pump fixture 및 active-zero/held/partial admission provenance 포함; 실제 음향은 별도 |
| E4 | [전용 collector 계약](REQ__native-input-collector.md), 닫힌 child의 독립 local QA 및 `target/wf/native-input-collector/qa-cli-2/workspace.log` | spawned owner/queue/cut/failure/join; 모든 OS 물리 입력을 뜻하지 않음 |
| E5 | [same-rate continuity 계약](REQ__native-output-continuity.md), 닫힌 child의 독립 QA·생산 owner/pump·ALSA null 검사 | 실제 same-rate recovery/prefix; converted/cross-backend/acoustic 증거 아님 |
| E6 | [workspace manifest](../../Cargo.toml), [app manifest](../../app/Cargo.toml), [CI 선언](../../.github/workflows/ci.yml) 직접 확인 | 선언·dependency 경계; hosted CI 성공 아님 |
| E7 | [MIT LICENSE](../../LICENSE), [ASIO 배포 계약](../platform/REQ__asio-distribution.md) | source/build 정책; 실제 release 감사 아님 |
| E8 | `selected-cohort-quic-development.log`: actual selected owner/header forwarding + worker join 1 PASS | 실제 production network owner이지만 peer handshake/교차 OS 소리 동기화 아님 |
| E9 | `pure-model-development.log`: `cargo check -p beatkernel-bms-runtime --no-default-features --lib --locked` PASS | read-only record/domain 모델의 graphics/browser 미의존 compile |
| E10 | 전체 WBS/집계 도구의 실제 ID·상태·기능 coverage 검사와 독립 bounded documentation-review PASS | 원본 §11.4/§22 누락을 수정한 문서 범위; combined code/security/QA PASS가 아님 |
| E11 | `quic-loopback-fixed-development.log`: real UDP/TLS QUIC loopback 4 PASS / 실패0 / ignored0 | compatible start/progress/final ACK, name/identity refusal, pending connect/accept 취소·join; WebTransport/교차 OS/물리 동기화는 별도 |
| E12 | `quic-ca-loopback-development.log`: 후속 real QUIC loopback 5 PASS / 실패0 / ignored0 | 별도 유효 unrelated CA 거절 추가; TLS 검증을 우회하지 않고 두 worker join |
| E13 | `controller-menu-fixed-app-development.log`: app lib 2027 PASS / 실패0 / ignored3; `browser-atomic-focused-development.log`: Node 집중214 PASS / 실패0 | mixed-rate rebind/cold end 준비·typed whole-owner lifecycle·genuine BKRP·atomic navigation; actual native launcher/GUI/browser audio 전체 완료 아님 |
| E14 | `webtransport-native-development.log`: owned HTTP/3 relay의 native room 4 PASS / 실패0 / ignored0, relay Ctrl+C exit0/join 확인 | 실제 roster/start/progress/final/drain·취소·identity/Origin/unrelated-CA; browser 상호운용/음향 동기화는 별도 |
| E15 | `inherited-target-owner-app-fixed-development.log`: app lib 2065 PASS / 실패0 / ignored4; `inherited-ui-wasm-development.log`: browser WASM check PASS | typed target controller/actual owner·pending retirement hold·공유 mapped pause·원본 source 시간·이동 후 드러나는 UI geometry/hit; 실제 launcher/GPU/browser QA 완료 아님 |
| E16 | `target-held-pump-bounded-app-development.log`: app lib 2072 PASS / 실패0 / ignored4; 선행 bounded 집중 shared-pump 4 PASS | 실제 공개 audio solo/cohort pump + real converted state + controlled 원본 target IO 관측으로 held/ACK/resume·교체 대기·실패 원자성 검증; 실제 launcher/driver/독립 QA는 별도 |
| E17 | `browser-component-motion-app-final-development.log`: app lib 2077 PASS / 실패0 / ignored4; `browser-all-component-motion-development.log`: Node 692 PASS / 실패0; production browser WASM build/bindgen/export smoke PASS | 실제 Selection/Display 메뉴 소유자·제출 pose/input·Back/zero/dispose·프레임/retry 분리의 pure/worker boundary; native motion/실제 GPU·browser QA는 별도 |
| E18 | `browser-software-motion-evidence.json`, `menu-motion-before.png`, `menu-motion-after.png`: actual regenerated WASM + renderer Worker + OffscreenCanvas + isolated SwiftShader Chromium | Selection SETTINGS가 x750→550 이동, 51 제출 프레임/오류0, 이전 좌표 거절/이동 좌표 control5 수락; 정상 dispose·브라우저 exit·서버 종료. 소프트웨어 기능 증거이며 hardware/native/full-flow/독립 QA는 별도 |
| E19 | `converted-ui-app-development.log`: app lib 2084 PASS / 실패0 / ignored4 | 실제 typed target owner 요청/응답·backpressure·cold refusal·전체 회수 및 ALSA target rate/sizes/matrix/applied metadata; 실제 Linux launcher/physical playback은 별도 |
| E20 | Linux solo consumer: lib 2086 PASS / 실패0 / ignored4; Linux bin 34 PASS / 실패0 / ignored2; all-bin check PASS; actual null functional 1 PASS | 실제 non-network solo converted 연결과 held 제출/회수/원본 BGM PCM; null은 PREPARED/played-frame 없음으로 원본 시계 admission 없음. clock-capable finite crossing/evdev/물리 전체 플레이는 별도 |
| E21 | `coherent-target-app-development.log`: lib 2093 PASS / 실패0 / ignored4, Linux bin 34 PASS / 실패0 / ignored2; `coherent-target-platform-development.log`: platform 131 PASS / 실패0 / ignored1 | 보조 telemetry 부재 시 원본 clock 보존·동일 generation cache·교체 유예·첫 native pair를 유지한 지연 end priming 검증; 물리 clock/전체 모드/독립 QA는 별도 |
| E22 | `linux-target-local-check-development.log`: Linux bin check PASS; `linux-target-local-bin-development.log`: 34 PASS / 실패0 / ignored3; `linux-target-local-null-development.log`: actual null local wrapper 1 PASS | offline local의 실제 converted owner 연결·held 제출·typed 관측/거절·전체 source 회수·원본 BGM cue; 실제 evdev/cohort 전체 플레이/native clock/network target startup는 별도 |
| E23 | `target-committed-app-development.log`: app lib 2099 PASS / 실패0 / ignored4; `target-committed-linux-check-development.log`: Linux bin check PASS | source/target 공통 시작 루프·실제 변환 상태 기반 시작 6건·solo/local 네트워크 target 소비자 연결; socket→driver/물리 동기화·target interval 지원·독립 QA는 별도 |
| E24 | `native-component-motion-check-development.log`: app binary check PASS; `native-component-motion-focused-development.log`: 8 PASS; `native-component-motion-bin-development.log`: 281 PASS / 실패0 / ignored3 | 실제 Desktop typed 명령·retained 노드·제출 pose 입력·캐시·Back/zero/clip/dispose 개발 fixture; 실제 native 창/GPU·하드웨어 성능·재리뷰/QA는 별도 |
| E25 | `native-component-motion-x11-development.log`: actual X11 window + llvmpipe Vulkan/Fifo 1 PASS | 실제 Desktop draw로 22 animation frame 제출·control5 -200px·이전/이동 입력 좌표·geometry identity/revision 유지·창/owner 종료; 하드웨어 성능·물리 입력·독립 QA는 별도. 종료 뒤 blank screenshot은 증거에서 제외 |
| E26 | `replay-target-app-consumer-development.log`: lib 2105 PASS / 실패0 / ignored4, replay bin 26 PASS / 실패0 / ignored1; `replay-target-null-development.log`: actual null 1 PASS | source/target 분리·typed replay pause/finite/idle drain·96 관측·실제 변환 상태 회수/원본 cue; clock-capable 전체 replay·다른 backend/OS·재리뷰/QA는 별도 |
| E27 | `browser-ime-roster-focused-development.log`: Node 175 PASS / 실패0 | 실제 main/Worker+WASM 경로의 composition/ACK·canonical roster·inventory 퇴역/재탐색·거절 회귀; 수정 후 trusted browser 재검증과 독립 QA는 별도 |
| E28 | `selected-quic-actual-development.log`: actual selected cohort 2 PASS; `selected-webtransport-actual-development.log`: actual selected room 2 PASS | 실제 admitted class/gauge/member identity의 peer start/progress/finals/drain·policy mismatch 거절·owner/relay 종료; full native room/음향 동기화·재리뷰/QA는 별도 |
| E29 | `browser-pagination-app-development.log`: lib 2107 PASS / 실패0 / ignored4; current WASM/bindgen PASS; `browser-pagination-all-node-development.log`: Node 707 PASS / 실패0 | 실제 row 수 기준 Players/Devices selection/page 정규화·21→10/20→5→empty shrink·cached compose·거절 회귀; 실제 브라우저 재검증·독립 재리뷰/QA는 별도 |
| E30 | `24b2569`의 독립 rank-metadata QA: `target/wf/qa-cli-bms-rank-01a11d56-1`; 전체 adapter 97 PASS / 실패0 / ignored0, 새 public rank fixture10, API docs PASS, code/security/docs review PASS | 실제 RANK/DEFEXRANK exact metadata·명시 precedence·조건/중복/원본 line·fabricated map·원래 판정 identity 검증. 기존 raw Clippy5와 lib/parser 포맷 차이는 실제 b097e42 baseline에서 재현; raw lint/format PASS 아님. timing preset/dynamic EXRANK/app profile 적용/전체 제품 완료는 별도 |
| E31 | `24663f3`의 독립 runtime report QA: `target/wf/qa-cli-runtime-report-01a11d72-2`; release example build/Clippy PASS, Python24 PASS, 실제6조합×3회18run report와 CLI 거절/보존/timeout/descendant 종료 및 Git index 읽기 전용 검사 PASS | 고정 software Runtime/queue/Mixer의 원본 per-child CPU/RSS·loop 측정·artifact와 unavailable 구분. 854240bytes는 benchmark 크기이며 player 크기 아님. Linux 실제실행, 다른 OS adapter는 테스트 근거; dense/UI stall/다인/GPU/soak/비교 및 전체 제품 성능은 별도 |
| E32 | `d0b062c`의 독립 input-disposition QA: `target/wf/qa-cli-input-disposition-01a11db1`; 전체 core417 PASS / 실패0 / ignored0(새 judge12/runtime8/replay6 및 doctest16 포함), core all-target Clippy PASS, desktop/webtransport app lib check PASS, DEEP code/security/docs PASS | 실제 판정 facts·정규화 거절·성공 prefix·replay 기록 후 callback·legacy hash/codec parity. 동일 v1 benchmark18회에서 연산/프레임/PCM checksum/counter 일치; 시간은 관측이며 성능 우위/zero-overhead 보증 아님. 최초 full-test compile240초 timeout 후 동일 전체 명령 cache continuation PASS. app 경고20·물리 검증·08.11 정책은 별도 |
| E33 | `891eb85`의 독립 dense-chart QA: `target/wf/qa-cli-dense-chart-01a11dd7/REPORT.md`, `cli-results.json`; 전체 core426 PASS / 실패0(새 dense9 포함), all-target Clippy/release build/DEEP code/security/docs PASS, 실제 release CLI38건(거절28건) PASS | actual judge/replay/projector의 원본2N 기록·golden head/tail/provenance·fresh prefix 복원·narrow tail·geometry/storage 검사. 기본2만 노트 및10만 노트/64레인 실제 실행; 반복20h/week origin facts 일치. origin은 장시간 실행이 아니며 레인은 member가 아님. GPU/물리/native/soak/rebind/다인 및 전체13.05/13.08 완료는 별도 |
| E34 | 영상 BGA 독립 DEEP code review와 qa-cli/qa-browser PASS: `target/wf/bms-video/qa-cli/`, `target/wf/bms-video/qa-browser/evidence.json`; app2124 PASS/실패0/ignored6, external51 PASS(실제 FFmpeg2 포함), Node750 PASS, native encoded-to-Scene/GPU2 PASS, 실제 Chromium23 checks PASS | VFR/B-frame/nonzero origin/preroll/backseek/EOF/반복 활성화·예산/ACK/종료·실제 native/browser GPU 및 preview/live/results/history/replay/local2/reload. Windows/macOS 실제 실행·물리 오디오 지연·모든 코덱/배포 선택은 별도. 재현 계약은 REQ__bms-video. |
| E35 | retained practice 독립 DEEP code/security cycle4와 qa-cli PASS: `target/wf/qa-cli-gapless-01a1220c-1/`; core public debug/release 각27 PASS, 현재 app2250/0/6·native307/0/5, CLI7/7, Windows/macOS all-targets typing exit0 | 실제 Desktop→spawned worker→공통 pump→production factory→유일 converted owner의 loop/disable/scrub/F5·부분 target admission·원본 PCM/기록 검증. 실패 Stop 누적·2..4 cohort·subframe cut·양수 시작 등록/replay 검증 포함. Native 관측은 주입; stub은 타입만 증명. 물리 음향/ASIO SDK·장치/브라우저 parity·출력 교체는 별도. |
| E36 | `f3e7c8f`의 독립 six-style DEEP code review와 qa-cli PASS: `target/wf/qa-cli-six-patterns-01a12342-1/`; core498 PASS/실패0/ignored0(doctest16 포함), release focused38/38, strict core all-target Clippy·4파일 포맷·CLI12/12 PASS | plan §22의 Absolute/Relative axis, dual contact, fresh press repeated, held chord trigger, pointer trajectory+button Instant, caller-owned orientation pose를 실제 Runtime/record/replay와 literal 결과/원본 metadata·negative·active checkpoint로 검증. 기존7개 예제 보존, 새 모드 각8개 hit. 실제 게임 호환·물리 장치·native/browser 실행·전체 제품 성능은 별도. |
| E37 | `01ed6a0` 소스에 대한 독립 code/security 리뷰 후 qa-cli 실행: `target/wf/qa-cli-records-01a1235f-1/`; app lib2295/0/6ignored, public replay17/0, native bins466/0/9ignored, standalone helper25/0, 실제 helper WASM Node3/0; model/browser check 및 Windows/macOS stub typecheck exit0 | 실제 개별 replay/archive staging·exclusive link·write/flush/sync/cleanup 오류·동시 게시·원본 오류/그룹 순서·예약 namespace·WASM typed refusal 검증. helper25는 lib에 포함된 반복 실행이다. profile 저장·프로세스 interruption/recovery·실제 disk exhaustion·hostile containment·directory power-loss·외국 OS 실행은 별도이며09.13은 V 유지. 전체 QA/close 절차와 실행된 테스트 결과를 구분한다. |

E38: `1920781`의 독립 DEEP code/security 리뷰 후 CLI QA 실행:
`target/wf/qa-cli-profiles-01a123b7-1/`; app lib2309/0/6ignored,
public profile10/0 및 replay17/0, native bins466/0/9ignored,
standalone helper28/0(앱 lib에 포함된 반복 검사), actual helper WASM Node3/0.
model/browser 빌드·Windows/macOS stub 타입 검사·Rust2021 변경 파일 포맷·diff
검사는 exit0이다. 최초 integration compiler guard124 후 같은 명령의 캐시
continuation은 exit0이었다. [프로필 저장 계약](../runtime/REQ__native-profile-publication.md)과
[재현 명령·측정 범위](../changes/2026-10-10-native-profile-publication-integrity.md)를 따른다.
실제 create/replace/reload·원본 오류/정리·예약 이름·동시 absent 분류를 검사했으며,
프로세스 interruption/recovery·실제 disk exhaustion·hostile containment·directory
power-loss·외국 OS 실행·direct public profile WASM 실행은 완료 근거가 아니다.
전체09.13은 V, 완료87/활성193을 유지하며 실행 결과와 Harness close를 구분한다.

E39: production source `fc60219`와 동일한 `5a9861c`의 실제 browser 및
browser-audio release 빌드·wasm-bindgen 생성은 exit0이다.
`target/wf/browser-record-result-continuity/current-packages-20261010/`의
main/audio SHA는 `3b8d338…` / `7235be08…`다. 독립 CLI는 240 handler 테스트와
native-free WASM dependency graph·pinned wasm-bindgen 0.2.129·u64 BigInt getter를
확인했다(`target/wf/qa-cli-browser-records-resume-20261010/`). 독립 headed browser
QA는 실제 6초 capture→save/use history/page→원래 timing/정확한 1/4/0/1 replay→
local keyboard/touch stop을 PASS했다
(`target/wf/qa-browser-records-tmpfs-cache-20261010/`). 실제 GPU 화면은 시각 검사됐고
owned browser/display/server/profile/cache 정리도 확인했다. 이 근거는11.01의
현재 package 빌드 기준을 충족한다. 오류 주입·모든 메뉴/장치·main-thread trace·
실제 음향/성능·다른 browser·full Goal 및 Harness close를 대신하지 않는다.
[검증 변경 기록](../changes/2026-10-10-browser-record-result-continuity.md)을 따른다.

E40: `703f7cc`의 Practice retained component/accepted-pose IME 연결은 독립
DEEP code review PASS, CLI 2,796 PASS/16 ignored 및 Node 67 PASS다.
현재 WASM의 실제 browser QA는 39 animation frames·이동 전후 클릭·editor repaint·
opacity refusal·Back/reentry와 screenshot 6장을 PASS했다. Native ordinary 7개와
명시 활성화한 실제 X11 window 1개도 PASS하고 직접 PNG를 검사했다. 다만 독립
desktop QA는 runtime X11 MCP가 같은 창을 열거·캡처하지 못해 BLOCKED_ENV다.
OS 입력/물리 IME·다른 화면·실제 하드웨어 성능 및 Harness close는 이 증거로
충족하지 않는다. [검증 기록](../changes/2026-10-10-practice-component-motion.md)을
따르며 10.16/10.17은 W, 전체 완료88/활성193을 유지한다.

E41: 현재 `6ac6769`/main WASM `197cadc2…`의 독립 browser timing QA는 실제
선택 프리셋 capture→history→replay의 점수 `1/4/0/1`, 네 프로필의16개 판정 행,
seek1.25/0, settings/legacy reset, missing-rank recovery, stop/retry 및
2인 keyboard/touch 기록을 PASS했다. Screenshot13장을 시각 검사하고 오류0,
owned browser/display/server/profile/cache cleanup을 확인했다.
기존 독립 code review 및 CLI96과 합쳐08.07의 소프트웨어 프리셋 연결 기준을
충족한다. [계약 및 현재 근거](REQ__bms-timing-presets.md#current-selected-policy-acceptance--2026-10-10),
`target/wf/bms-timing-resume-20261010/REPORT.md`를 따른다. 실제 장치/물리 성능·
전체 원본 엔진 호환·Harness close 및 전체 player 완료는 별도다.

E42: `de8ee17`/browser WASM `2fbf27ac…`의 실제 retained Records 연결은
독립 DEEP code review, CLI Rust 2,391 PASS/6 기존 ignored 및 Node 931 PASS,
실제 browser QA와 UX PASS를 확인했다. Genuine 완료 기록 두 개의 accepted prefix와
저장 comparison/history, Own/Other의 비영 selected count, duplicate refusal,
preview 없는 exact-kind Remove, unrelated preservation, Clear, exact Settings Back,
keyboard/empty catalog 및 save/delete/Scan의 literal selection 보존을 증명했다.
원래 11.05 기준의 독립 사실 감사도 승격을 권고했다.
[변경 및 검증 기록](../changes/2026-10-11-records-retained-actions.md)을 따른다.
QA1의 keyboard FAIL과 수정 뒤 QA2 PASS를 함께 보존한다. 모든 owned GUI 자원은
종료·회수했다. 물리 장치/성능, 전체 UI 및 receipt-backed Harness close는 별도다.

E43: `87c8ba9`/검증 문서 `531877c`의 dense report는 독립 DEEP code/security,
CLI 및 문서 리뷰 PASS 후 Harness 검증·close를 통과했다. Python44 PASS와 현재
소스 release 빌드, 실제2만/10만 노트의 두 조합×3회 CPU/RSS/artifact report,
추가 stdout2회 및 CLI14경로를 검증했다.
`target/wf/dense-chart-benchmark-qa-12be8-1/`과
[측정 범위·재현 기록](../changes/2026-10-12-dense-chart-benchmark-report.md)을 따른다.
전체 자식 프로세스 RSS는 기록/checkpoint/독립 검증을 포함하며 player 메모리가
아니다. Git 정보는 기존 제한 시간 fallback으로 null이며 별도 source/build
identity를 보존했다. 64레인은64명이 아니고 GPU/물리/장시간/전체 player 성능 및
원본13.05/13.08은 미완료다. 이 근거만으로 완료 개수를 올리지 않는다.

E44: `1712136`의 [실제 cohort transition workload](REQ__cohort-transition-stress.md)는
production solo/cohort pump, original member capture, retained Mixer PCM과
pause/replacement policy를 사용한다. 독립 DEEP code/security PASS 다음 fresh
CLI QA11개 테스트·release 빌드·30실행(정상/경계12, 잘못된 입력18)을 통과했다.
1/2/3/4/64명 및 반복 attempt, 별도 cycles64/notes64 경계와 byte-identical
64명 재실행을 검증했다. `target/wf/cohort-transition-stress-qa-12bfe-1/`과
[개발 실패·독립 검증 기록](../changes/2026-10-12-cohort-transition-stress.md)을 따른다.
Practice 중 output replacement는 실제 application 거절을 확인했고, 성공 rebind는
별도로 허용된 paused composition이다. 물리/native worker retirement, GPU/network,
전체20h/week timeline·wall-clock soak 및 성공 interleaving을 증명하지 않는다.
원본13.05/13.08 및 완료91/193은 유지한다.

E45: `a61b03a`의 [연속 장시간 player workload](REQ__long-duration-player-traversal.md)는
독립 DEEP code/security PASS 뒤 fresh QA6/6·최적화 빌드·실제 CLI36건
(정상/도움말/경계12, 거절24) 및 전체20h/week 순회를 통과했다.
원본 production pump/Mixer/capture로 시작점0부터72000000/604800000 재생
프레임과 실제 held3프레임의 PCM을 모두 순서대로 검사했다. Week 재실행은
stdout이 동일하고 실제 입력336개/판정252개 및 종료 큐 비움을 확인했다.
최종 문서 리뷰와 Harness verify/close는 아직 대기다. 고정1000Hz sparse software
검사로 실제 표준-rate/native/물리 latency·wall-clock soak·전체 player 자원
안정성을 증명하지 않으며13.05/13.08과 완료91/193을 유지한다.
[개발 원본과 검증 기록](../changes/2026-10-12-long-duration-player-traversal.md)을 따른다.

E46: `3ecd1bbb`의 original-coordinate authority epoch 준비·두 원본 관측 priming·
입력 merger 재검증은 `ac001-authority-fixed.log`에서214 PASS/실패0이다.
`ac001-input-regression.log`의31 PASS는 **기존 입력 회귀**이며 새 테스트31개가 아니다.
커밋된 [authority 구현](../../app/src/audio_authority_coordinate_epoch.rs)과
[epoch fixture](../../app/src/audio_authority_coordinate_epoch_fixtures.rs),
[authority 계약](REQ__audio-authority.md)을 함께 따른다. native IO·controller·
실제 retained 출력 교체 완료 근거는 아니다.

E47: `9e224d61`의 retained Target native presentation 및 원래 Mixer pin 보존은
성공한 `ac002-native-presentation-green.log`의67 PASS/실패0으로 확인했다.
앞선 실패 `ac002-native-presentation-final.log`를 PASS 근거로 쓰지 않는다.
커밋된 [presentation 구현](../../app/src/native_audio_presentation_coordinate_epoch.rs)과
[epoch fixture](../../app/src/native_audio_presentation_coordinate_epoch_fixtures.rs),
[원본 native 증거 계약](REQ__native-presentation-validation.md)을 따른다.
원본 관측을 공급한 software owner 검사이며 launcher/device 실행과 다르다.

E48: `df70e2e5`의 coordinate pause rebind·frozen adoption·held generation 검사는
`ac003-pause.log`에서75 PASS/실패0, 포맷 후 집중 재실행
`ac003-pause-formatted.log`에서13 PASS/실패0이다.13을 별도 새 기능 수로 합산하지 않는다.
커밋된 [pause 구현](../../app/src/playback_pause_coordinate_epoch.rs)과
[실제 Mixer/converter fixture](../../app/src/playback_pause_coordinate_epoch_fixtures.rs),
[retained coordinate 계약](REQ__retained-playback-speed.md)을 따른다.
원래 source grid·history·pause 보존 범위이며 실제 공통 pump/UI 교체는 남는다.

E49: `1463ccfa`의 sole converted worker conditional retirement는
`ac008-conditional-retirement.log`에서16 PASS/실패0,
`ac008-platform-final.log`에서 전체 platform387 PASS/실패0/ignored2,
명시적으로 opted-in한 `ac008-native-null.log`에서 actual ALSA null1 PASS다.
커밋된 [worker 결정 경계](../../crates/beatkernel-platform/src/linux/alsa/converted_conditional_retirement.rs),
[owner/worker fixture](../../crates/beatkernel-platform/src/linux/alsa/converted_conditional_retirement_fixtures.rs),
[정적 port](../../app/src/gameplay/output/ports.rs)와
[ALSA adapter](../../app/src/gameplay/output/adapters/alsa.rs),
[complete-owner 계약](REQ__native-output-continuity.md)을 따른다.
변경된 해석의 pending suffix 거절·cancel/authorize·회수/retry software lifecycle에
한정하며 null은 물리 소리·지연·retained GUI 성공 증거가 아니다.

E46–E49 로그 위치는 ignored `target/wf/retained-practice-output-replacement/`다.
이 네 기초 D는 해당 커밋의 범위 한정 개발 검사를 뜻한다. 현재 parked task의
formal review/QA 또는 전체07.08 완료를 대신하지 않는다.

E50: Linux encoding의 `18d9c414`는 [Linux encoding 계약](../platform/REQ__linux-output-encoding.md),
[커밋·재현 명령·독립 실행 기록](../changes/2026-10-13-linux-output-encoding.md)에
따라 독립 code review/CLI QA의 app3676 PASS/실패0/ignored26과 두 opted null
검사를 통과했다. 커밋된 [설정/profile fixture](../../app/tests/output_encoding_settings.rs),
[shipped CLI fixture](../../app/tests/native_output_encoding_cli.rs),
[ALSA UI fixture](../../app/src/gameplay/output/adapters/alsa_ui_encoding_fixtures.rs),
[live](../../app/src/bin/linux_bms/output_encoding_fixtures.rs)/
[replay fixture](../../app/src/bin/play_replay_bms/output_encoding_fixtures.rs)를 따른다.
actual desktop Settings edit/refusal/Apply·profile restart·129-row 경계도 확인했다.
Linux 설정/profile/CLI 및 adapter/cold-start/replay software 범위만 D이며,
acknowledged-paused Output GUI는 evdev 부재로 BLOCKED_ENV였다. foreign Rust
stub typing·null·helper로 실제 SDK/다른 OS/물리 음향·retained 교체를 완료 처리하지 않는다.

E51: AC004 coordinate controller 후보는 앞선
`ac004-coordinate-target-fixed.log`의18개 중15 PASS/3 FAIL 이후 fixture를
수정했고, 재개 후 실제 `ac004-coordinate-target-resumed.log`에서18 PASS/실패0을
확인했다. borrowed commit 변경 후 `ac004-borrowed-placement-unit.log`에서도
18 PASS/실패0이다. 별도 integration의 `ac004-borrowed-allocation-calibrated.log`는
실제 nonempty registry를 가진 prepared token의 clone이 할당함을 먼저 확인하고,
실제 held publication 구간의 할당/재할당0회를 검사해1 PASS다. 단위 계측기의
unsafe 금지 규칙 컴파일 실패9건은 앞선 로그에 보존하고, 라이브러리 규칙을 유지한 채
같은 안전한 fixture를 공유하는 외부 테스트 실행 파일로 계측을 옮겼다.
현재 [controller 개발본](../../app/src/gameplay/output/application/coordinate_target_replacement.rs)과
[fixture 후보](../../app/src/gameplay/output/application/coordinate_target_replacement_fixtures.rs)는
software 범위만 확인되어 scoped V로 추적한다. 새 retained typed Owner/UI(AC005)는
자체 구현·검사를 마쳐 E52의 V로 추적한다. 실제 공통 pump(AC006)는 구현·통합
테스트를 구현하고 실제 연결 시나리오와 pure app 회귀를 통과해 E53의 V로 추적한다.
controller PASS를 후속 AC의 근거로 물려주지 않는다.
`ac004-borrowed-regression-*.log`의
output application162 / authority214 / native presentation67 / pause75 / input31도
각각 PASS/실패0이며 서로 겹치는 필터 수를 고유 테스트 수로 합산하지 않는다.
불필요한 merger metadata clone은 제거됐고 최종 독립 review/QA가 남았다.
이 검사들을 후속 AC의 실행 결과나 formal current-task review/QA로 옮기지 않는다.

E52: AC005 typed Owner/UI는 실제 원래 owner를 conditional 대기 중 노출하고 실제
authorization 뒤 기존 clock을 중단하며, capability를 commit 전에 검증해 동일
요청 ID의 응답을 WouldBlock에도 한 번만 전달하도록 구현했다.
`ac005-retained-target-formatted.log`는17 PASS/실패0(새 request8·observation8과
기존1)이며 output application178, controller18, ALSA UI12 PASS·ignored2,
별도 실제 publication 할당 회귀1 PASS, Linux bin53 PASS·ignored3,
replay bin40 PASS·ignored1 및 no-default 모든 bin check도 통과했다.
각 필터는 겹치므로 고유 테스트 수로 합산하지 않는다. 로그는 같은 task의
`ac005-regression-*.log`, `ac005-allocation-regression.log`,
`ac005-linux-replay-regression.log`, `ac005-bins-check.log`에 있다.
앞선 fixture 설정 형식 실패와 root 부분 포맷의 중복 import 컴파일 실패도 보존했다.
현재는 software V이며 실제 solo/cohort 연결·GUI/물리 증거·최종 독립 gate는 별도다.

E53: AC006은 실제 solo 및2인 RuntimeGroup 공통 pump의 입력 수집 직후 pause
입력 drain·교체 poll을 연결했다. `ac006-connected-fixture-fixed.log`의3개 실제
사례는 same/changed rate·channel/matrix, Pending/Authorized/Refused, held suffix,
입력 수거·조기 resume fence, 실제 loop/scrub/section end와 PCM을 통과했다.
교체 전 Source fingerprint를 publication/거절에서 무조건 비교하고 archive의
실제 encoded/decode 기록을 live accepted input/advance·JudgeEvents·member/attempt
메타데이터와 비교하며 최종 live member hash도 일치했다.
`ac006-app-lib-no-default.log`는2674 PASS/실패0·ignored6,
`ac006-allocation-regression.log`는1 PASS/실패0으로 실제 publication 새 할당0회다.
이 필터는 서로 겹치며3개를2674개에 다시 더하지 않는다.
`ac006-desktop-app-bins.log`는 desktop app lib3129 PASS·ignored8 및 각 bin
393/53/29/40/3/7/59 PASS·ignored 합계17로 실패0이고,
`ac006-desktop-bins-check.log`의 모든 bin check도 통과했다.
최종 독립 review/QA·실제 GUI/물리 clock 증거는 아직 남았다.
앞선 compile5errors와 fixture1 PASS/2 FAIL은 원본 로그에 보존했다.

E54: `015256a3`의 독립 DEEP 리뷰는 history-retirement 경합1건으로 FAIL했다.
`review-history-red.log`는 기존 API에서 실제0 PASS/1 FAIL(exit101)을 재현했다.
공통 Owner의 conditional history 서비스와 두 Linux delegate를 수정한 뒤
`review-history-green.log`는 요청/관측25 PASS(새 이력 검사8개 포함),
`review-history-connected.log`는 실제 Owner history port를 연결한 solo/cohort와
전환상태3 PASS다. 두 pump는 실제 old packet 뒤 request authorization을 각각
겪고도 교체·resume·loop/scrub·기록 검사를 계속 통과한다.
`review-history-full-app.log`는2682 PASS/실패0·ignored6,
`review-history-allocation.log`는 실제 publication 새 할당0회1 PASS다.
38개 Task Rust 파일의 프로젝트 포맷과 scoped diff도 통과했다.
07.09.03/07.08.06은 개발 재검사 후 V로 돌아가며 최종 재리뷰/CLI/desktop QA와
실제 물리 clock 증거는 별도다. 앞선25/3 필터를2682개에 더하지 않는다.

E55: `095a1eb3`의 독립 DEEP 재리뷰 PASS 뒤 CLI QA PASS를 받았다.
실제 desktop/WebTransport lib3166/0/8ignored, 각 bin 합계584/0/17ignored,
platform387/0/2ignored, 외부 실제 publication 할당1 PASS다. WASM browser-only
lib check·38개 Rust 포맷·diff 검사 및 Windows/macOS stub Rust typing은 exit0이다.
Clippy exit0에는 app348/platform109/BMS5/kernel2 non-denied 경고가 남는다.
실제 CLI help/거절 및3초48kHz mono144000frame 비영 PCM 렌더도 확인했다.
독립 desktop은 실제 :99 창의 Settings44.1kHz Apply/reopen·키보드·resize·solo/local
Start까지 interactive-desktop을 실행했으나 /dev/input,/dev/uinput,/dev/snd가 없어
native 재생 시작 전에 막혔다. acknowledged pause/Output/resume/capture는 미검증으로
BLOCKED_ENV다. 두 owned 창/process는 종료했다. 자동 완료 기록도 없으므로 실제
review/CLI 결과는 NON-ATTESTING이며 task close/전체 Goal 완료가 아니다.
실제 장치 blocker를 보존하고 기존 V와 완료97/206·원래91/193을 유지한다.
자세한 재현·경고·한계는 [변경 기록](../changes/2026-10-14-retained-practice-output-replacement.md),
ignored qa-cli-095a1eb3/ 및 qa-desktop-095a1eb3/에 있다.

E1의 재현 명령(기존 [도구 설정 지침](../common/GUIDE__parallel-development.md)과
현재 host compiler 환경을 먼저 사용한다):

```sh
cargo test -p beatkernel-bms-runtime --features desktop,webtransport --lib --locked
cargo test --workspace --locked
cargo check -p beatkernel-bms-runtime --no-default-features --lib --locked
```

QUIC 실제 4-case loopback은 Tokio context 결함 수정 후 E11로 통과했다.
`BEATKERNEL_TEST_QUIC_CERT/KEY/CA/SERVER_NAME`을 실제 owned PKI로 제공하고
`--test multiplayer_quic_loopback -- --ignored --test-threads=1`로 재현한다.
이 결과는 native QUIC 해당 사례에 한정되며 WebTransport/교차 OS나 물리
동기화를 대신하지 않는다.

## WBS

각 leaf 문장 자체가 관찰 가능한 완료 기준이다. 근거가 없는 행의 뒤쪽은
필요한 증거 또는 다음 행동이며 PASS 주장이 아니다.

### 01. 아키텍처·워크스페이스·개발 기반

기능 연결: BK-022, BK-044, BK-054, BK-058, BK-059. 요구: 원본 plan §§2–4, 18–20, 24.

- [x] **BK-WBS-01.01** 코어/플랫폼/BMS 어댑터/app의 실제 crate 경계를 유지한다. — 상태=D(검증완료); 선행=-; 근거=E6.
- [x] **BK-WBS-01.02** 코어에는 OS API와 게임 이름 분기가 없고 플랫폼·어댑터는 상호 의존하지 않는다. — 상태=D(검증완료); 선행=-; 근거=E0,E6.
- [x] **BK-WBS-01.03** UI·설정·기록·경쟁·네트워크를 하나의 app crate에서 조립한다. — 상태=D(검증완료); 선행=-; 근거=E6.
- [x] **BK-WBS-01.04** Rust 1.98.1/MSRV와 CI 선언을 맞추고 Cargo.lock 기반 빌드가 가능하다. — 상태=D(검증완료); 선행=-; 근거=E6,E9.
- [ ] **BK-WBS-01.05** 모든 application context를 DDD 모듈과 명시적 Port/Adapter로 분리한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=gameplay/output 이후 나머지 context 의존성 audit.
- [ ] **BK-WBS-01.06** UI 제어·기록·파일·장치·시간·네트워크 효과를 주입 가능하게 만든다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=실제 adapter를 교체한 계층별 fixture.
- [ ] **BK-WBS-01.07** hot path의 static dispatch와 추상화 비용을 실제 코드·측정으로 확인한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=동적 dispatch/clone/alloc 위치와 비용 보고.
- [x] **BK-WBS-01.08** 순수 기록 모델을 graphics/browser 없이 빌드한다. — 상태=D(검증완료); 선행=-; 근거=E9.
- [ ] **BK-WBS-01.09** 미지원 기능은 capability/구체 오류로 표시하고 장치·모드·규칙을 몰래 대체하지 않는다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=native/browser capability 전체 경로 검사.
- [ ] **BK-WBS-01.10** 독립 작업의 파일 소유권·선행 관계·통합·검증 장벽을 유지한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=병렬 작업 및 최종 통합 audit.

### 02. 정수 시간·Transport·오디오 기준 시계

기능 연결: BK-001, BK-002, BK-021, BK-034. 요구: plan §§5, 17.1–2; REQ__native-target-time.

- [x] **BK-WBS-02.01** Timestamp/Duration 산술·음수·i64 경계 오류가 상태를 부분 변경하지 않는다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-02.02** 1x/0.5x/2x 및 1,000회 이상의 rate 변경이 기준 anchor 계산과 일치한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-02.03** pause/seek/reverse에서 원래 시간·이벤트 순서·snapshot 의미를 보존한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-02.04** 20시간/1주일 구간·큰 절대 좌표를 정수로 처리하고 표현 불가 범위는 거절한다. — 상태=D(검증완료); 선행=-; 근거=E0,E1,E2.
- [x] **BK-WBS-02.05** 다른 target rate 구간의 정확한 유리수 시간을 누적하고 마지막에 한 번만 floor한다. — 상태=D(검증완료); 선행=-; 근거=E2,E3.
- [x] **BK-WBS-02.06** 원본 clock domain/provenance를 보존하고 unknown mapping으로 판정을 진행하지 않는다. — 상태=D(검증완료); 선행=-; 근거=E0,E1.
- [ ] **BK-WBS-02.07** 실제 native/browser 플레이의 master clock을 출력 오디오 관측에 일관되게 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=source pull/HOST 시계를 출력 진행으로 대신하지 않는 E2E.
- [ ] **BK-WBS-02.08** backend/rate/buffer/latency 변경 시 epoch·입력 mapping·song origin을 원자적으로 갱신한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=실제 교체 경로·실패/재시도·다른 backend 검사.
- [ ] **BK-WBS-02.09** 동일한 정수 판정 시간과 시각 보간 시간을 분리해 장시간 시각 drift를 검사한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=[장시간 시각 계약](REQ__long-duration-visual-time.md): 큰 과거 SV 및 마커 교차 정밀도 결함을 indexed 상대 구간 합으로 수정. 실제 20시간/1주일 가상 traversal·Runtime/replay 정수 identity·app retained geometry는 독립 코드 리뷰·CLI QA PASS(코어 전체 451, 신규 코어 8, app 3 및 기존 playfield 3); 실제 GPU/physical drift 증거는 남음.
- [ ] **BK-WBS-02.10** 향후 pre-play 입력 보존/폐기 규칙을 확정하고 metadata/recording 의미를 문서화한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=기존 미응답 정책 질문; 현재 동작 보존.

### 03. Canonical 입력·binding·수집

기능 연결: BK-003, BK-004, BK-012, BK-015, BK-016, BK-018, BK-036, BK-037, BK-052. 요구: plan §6; REQ__native-input-collector.

- [x] **BK-WBS-03.01** 동일 control도 DeviceId가 다르면 독립 입력으로 처리한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-03.02** Button/Axis/Touch/Pointer/Pose/RawHID/Custom 의미와 payload를 보존한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-03.03** native timestamp/원본 metadata/sequence가 정규화·mapping·replay에서 보존된다. — 상태=D(검증완료); 선행=-; 근거=E0,E4.
- [x] **BK-WBS-03.04** canonical 입력 codec의 왕복·잘못된 길이·범위·용량 거절을 검증한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-03.05** exact-device/any-device binding과 두 장치의 동일 키 fanout을 검증한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-03.06** Windows/Linux/macOS 표준 키 fixture가 같은 HID Usage로 수렴한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-03.07** raw HID report를 실제 DeviceAdapter port로 button/axis에 변환하고 malformed report를 거절한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-03.08** 전용 입력 collector가 게임/UI/audio owner와 독립적으로 획득하고 종료 시 join한다. — 상태=D(검증완료); 선행=-; 근거=E4.
- [x] **BK-WBS-03.09** FIFO acquired-prefix·완료 cut·overflow·지연 이벤트·취소/cleanup 실패를 보존한다. — 상태=D(검증완료); 선행=-; 근거=E4.
- [ ] **BK-WBS-03.10** 실제 장치 hotplug/분리/권한 거절을 선택 identity와 함께 처리한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=OS별 실제 장치 및 fallback 거절.
- [ ] **BK-WBS-03.11** 1인 자동 입력과 2..64인 명시 할당을 native/browser 실제 시작에 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=UI roster와 native source identity E2E.
- [ ] **BK-WBS-03.12** 키보드·HID·고주파 입력의 실제 OS timestamp 의미와 손실/순서를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=Windows/Linux/macOS 장치·1kHz 이상 source.

### 04. 채보 compiler·판정·공통 Runtime

기능 연결: BK-005, BK-006, BK-007, BK-023, BK-028, BK-030. 요구: plan §§7–9, 13–14, 17–18, 22.

- [x] **BK-WBS-04.01** BPM 변경·STOP·SV를 판정/시각 timeline으로 분리하고 golden 결과를 유지한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-04.02** 동일 source/seed가 동일 object ID·동시 이벤트 순서·chart identity를 생성한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-04.03** Instant/Hold의 head/tail·inclusive asymmetric 경계와 input offset 한 번 적용을 검증한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-04.04** Tracking/Repeated/Composite 및 custom 후보/등급 정책을 실제 fixture로 검증한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-04.05** contact ownership/release/cancel/rebind와 relative/absolute axis 규칙을 구분한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-04.06** 반복 Down/Repeat/Up이 새로운 builtin press를 임의로 만들지 않는다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-04.07** virtual 입력→binding→judge→audio 명령의 실제 순서/결과를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-04.08** 독립 local RuntimeGroup의 member 결과·voice 영역·source identity를 보존한다. — 상태=D(검증완료); 선행=-; 근거=E0,E1.
- [x] **BK-WBS-04.09** 게임/OS 분기 없이 최소 네 가지 일반화 입력·상호작용 fixture를 실행한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [ ] **BK-WBS-04.10** 모든 오류·capacity 초과·trusted callback 경계를 계층별로 다시 audit한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=오류 전후 상태·panic/side-effect 계약.
- [ ] **BK-WBS-04.11** 현재 source 전체에서 live class/EX/accepted lane 피드백을 실제 GUI와 일치시킨다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 Runtime prefix→UI→결과 비교.
- [x] **BK-WBS-04.12** SDVX axis·Arcaea dual-contact·Taiko repeated·GITADORA composite·osu pointer·VR pose 여섯 일반화 fixture가 각각 실제 입력/판정 결과를 재현한다. — 상태=D(검증완료); 선행=BK-WBS-04.04; 근거=E36, [여섯 조합 계약](REQ__generalization.md); 범위=원본 plan §22의 대표 software 조합, 실제 게임 호환/장치 검증 아님.

### 05. 오디오 커널·PCM·명령·변환

기능 연결: BK-009, BK-010, BK-017, BK-019, BK-024, BK-027. 요구: plan §11; REQ__audio / native-output-continuity.

- [x] **BK-WBS-05.01** 미리 준비한 SampleBank/PCM을 bounded Mixer voice에서 출력한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-05.02** 키음/BGM 명령의 sample offset·gain·동시성·credit/ACK·overflow를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E0,E1.
- [x] **BK-WBS-05.03** 명시적 channel matrix와 resampler 품질·history/phase 보존을 검증한다. — 상태=D(검증완료); 선행=-; 근거=E0,E2,E3.
- [x] **BK-WBS-05.04** 변환 source pull/consumed phase/target generation/admission을 다른 사실로 취급한다. — 상태=D(검증완료); 선행=-; 근거=E2,E3.
- [x] **BK-WBS-05.05** same-rate pending PCM과 partial native write의 정확한 suffix를 owner가 보존한다. — 상태=D(검증완료); 선행=-; 근거=E5.
- [x] **BK-WBS-05.06** converted pending PCM·admission offset·retarget 거절이 converter 전체 상태를 보존한다. — 상태=D(검증완료); 선행=-; 근거=E3.
- [x] **BK-WBS-05.07** held로 생성된 pending PCM provenance를 유지하고 active 무음과 구별한다. — 상태=D(검증완료); 선행=-; 근거=E3 후속128-test 실행의 실제 owner 2개 provenance fixture; partial prefix와 cold failure 보존 포함.
- [x] **BK-WBS-05.08** 생산 PCM admission loop가 positive prefix를 먼저 확정하고 뒤 telemetry 오류를 분리한다. — 상태=D(검증완료); 선행=-; 근거=E3,E5.
- [x] **BK-WBS-05.09** source pause/resume/start/end를 target 경계로 매핑해 cached/held/coalesced report에도 보존한다. — 상태=D(검증완료); 선행=-; 근거=E2,E3,E1.
- [x] **BK-WBS-05.10** startup source gate를 한 번 올림하고 실제 PCM onset/target 경계와 일치시킨다. — 상태=D(검증완료); 선행=-; 근거=E1: 44.1→48kHz, 589µs→source26/target29.
- [ ] **BK-WBS-05.11** 실제 backend callback 성공/오류 경로의 alloc/dealloc/lock/I/O를 audit한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=준비와 callback 경계를 포함한 계측.
- [ ] **BK-WBS-05.12** 미지원 format/channel/rate/period와 native 쓰기 실패를 구체 오류로 반환한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=모든 backend 실제 applied-config 검사.
- [ ] **BK-WBS-05.13** pitch 변화 허용 배속의 sample read-head/resampling과 명령 rate가 선언한 재생 속도에 맞는 PCM을 만든다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-05.01; 필요 근거/다음=plan §11.4; [offline replay 배속 독립 검증](../changes/2026-10-12-replay-render-playback-speed.md)은 shared planner·실제 PCM·DEEP code/security PASS·QA40테스트/optimized CLI54경로까지 확인; [고정 배속 common/native/browser replay 독립 검증](../changes/2026-10-12-realtime-replay-speed.md)은 DEEP code/security·CLI·실제 browser/UX PASS 및 WASM·Worker·Worklet·renderer 네 배속과 native pause/null/외국 OS 타입 검사를 확인; null startup은 자연종료/음향 증거가 아님; live/runtime 배속 전환·retained practice·물리 출력의 속도·시계·pause/seek 연동도 남음; device sample-rate 변경을 playback speed로 대신하지 않음.
- [ ] **BK-WBS-05.14** ReverseTimelineOnly/ReverseSamples/Mute 선택 정책의 명령·원본 sample 방향·무음 결과를 검증한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-09.02; 필요 근거/다음=core runtime/playback.rs 및 reverse_playback fixture를 현재 scope로 확인; Transport reverse만으로 sample 정책을 완료 처리하지 않음.

### 06. 플랫폼별 Native IO backend

기능 연결: BK-012, BK-013, BK-014, BK-015, BK-016, BK-017, BK-057. 요구: plan phases4/7/12/13; platform REQ.

- [ ] **BK-WBS-06.01** Windows Raw Input 키보드/HID 획득과 native owner 종료를 최신 source로 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=현재 Windows 실행; 과거 VM 결과만 확대 인용 금지.
- [ ] **BK-WBS-06.02** Windows WASAPI shared stream·format/period/device/clock 제어를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 드라이버/applied-config/회수.
- [ ] **BK-WBS-06.03** Windows WASAPI exclusive stream·지원 거절·buffer 크기를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 exclusive device matrix.
- [ ] **BK-WBS-06.04** 선택적 ASIO SDK/MSVC bridge·채널/rate/buffer granularity·callback/clock을 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 SDK 빌드와 설치된 신뢰 driver.
- [ ] **BK-WBS-06.05** Linux evdev 표준 키·장치 identity·분리/권한·drain을 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 evdev 장치.
- [ ] **BK-WBS-06.06** Linux hidraw report 획득·adapter·손실/종료를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 HID 장치.
- [ ] **BK-WBS-06.07** ALSA device/format/period/buffer·timestamp·xrun·완전 회수를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 출력 장치; null은 음향 증거 아님.
- [ ] **BK-WBS-06.08** macOS IOHIDManager/checked queue와 모든 선택 source의 drain을 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 키보드/HID와 runloop/queue 오류.
- [ ] **BK-WBS-06.09** CoreAudio device/format/buffer·callback·presentation interval·회수를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 macOS 출력 장치.
- [ ] **BK-WBS-06.10** 각 OS default device와 명시 override를 자동/정확 선택 계약으로 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=native 장치 선택과 unavailable 오류.
- [ ] **BK-WBS-06.11** format/rate/channel/latency/period capability와 실제 적용 결과를 공통 UI에 표시한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=요청과 applied 차이를 숨기지 않는 검사.
- [ ] **BK-WBS-06.12** Windows+Linux의 실제 입력→판정→소리 및 current binary 동작을 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=원본 최소 두 OS 완료 조건; 장치/VM 제공.
- [ ] **BK-WBS-06.13** macOS 실제 전체 플레이·입력·오디오 실행을 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=macOS host/device.
- [ ] **BK-WBS-06.14** ASIO 실제 driver로 재생·pause/end·종료·buffer 변경을 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=Windows/MSVC/ASIO driver.
- [ ] **BK-WBS-06.15** 추가 출력 API 확장 경계와 지원/미지원 목록을 문서·실제 capability에 맞춘다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=추가 API는 구체 계약/증거 필요; 모든 이름 지원으로 오인 금지.

### 07. 실제 플레이·재생·출력 교체·연속성

기능 연결: BK-017, BK-019, BK-020, BK-021, BK-027, BK-033, BK-035, BK-043. 요구: REQ__native-target-time; selected-policy-network.

- [x] **BK-WBS-07.01** 고정 rate 출력 retirement/recovery에서 whole-owner와 immutable source grid를 보존한다. — 상태=D(검증완료); 선행=-; 근거=E5.
- [x] **BK-WBS-07.02** source/target 분리된 native presentation/start/pause/resume/end 기초 helper를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E1; 실제 launcher 완료는 아래 별도.
- [x] **BK-WBS-07.03** target pause/end rebind에서 source-frame floor와 정확한 target-time floor를 분리한다. — 상태=D(검증완료); 선행=-; 근거=E13 actual mixed-rate/retained-tail/planned basis/frozen source/native ACK/cold finite-end helper 회귀; controller publication은 07.08 별도.
- [x] **BK-WBS-07.04** 기존 출력 lifecycle/controller를 typed target basis로 확장하고 두 번째 runtime을 만들지 않는다. — 상태=D(검증완료); 선행=-; 근거=E15; static generic port·기존 legacy default 회귀·실제 converted owner 및 실패/회수/hold 보존; launcher 연결은 아래 별도.
- [ ] **BK-WBS-07.05** ConvertedAlsaStream을 실제 solo 시작·입력 AudioAuthority·BGM 공급에 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E20 solo, E21 telemetry, E22 local, E23 network target startup 연결·pure 검증; clock-capable endpoint 전체 플레이 및 socket→driver 시작 검증 필요.
- [ ] **BK-WBS-07.06** 같은 converted 출력 owner를 실제 local cohort 경로에 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E22/E23 actual local consumer·null/공유 pump 검증; clock-capable 전체 cohort·member ID/clock/capture/section E2E 및 독립 QA 필요.
- [ ] **BK-WBS-07.07** 실제 recorded playback을 target rate와 source recording grid가 분리된 경로에 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E26 실제 consumer 연결·pause/finite/idle drain·null 회수/원본 cue 검증; clock-capable 전체 replay와 다른 backend/OS·독립 QA 필요.
- [ ] **BK-WBS-07.08** paused output 교체의 open/start/poll/prime/commit을 입력 merger와 원자적으로 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=2개 실제 native progressing 관측; stale publish 상태 보존.
  - [x] **BK-WBS-07.08.01** original-coordinate authority epoch 준비·priming·입력 merger 재검증 기초를 구현하고 검사한다. — 상태=D(검증완료); 선행=-; 근거=E46, 3ecd1bbb authority214 PASS와 기존 input31 PASS; 범위=authority/merger software 경계, 실제 출력 교체는 별도.
  - [x] **BK-WBS-07.08.02** retained Target native presentation과 원래 Mixer creation pin을 보존하는 epoch 기초를 구현하고 검사한다. — 상태=D(검증완료); 선행=BK-WBS-07.08.01; 근거=E47, 9e224d61 successful green67 PASS; 범위=원본 관측 software owner, launcher/device는 별도.
  - [x] **BK-WBS-07.08.03** coordinate pause rebind·frozen adoption·held generation 기초를 실제 Mixer/converter로 검사한다. — 상태=D(검증완료); 선행=BK-WBS-07.08.02; 근거=E48, df70e2e5 pause75 PASS와 포맷 후 집중13 PASS; 범위=원래 source grid/history/pause 보존, pump/UI는 별도.
  - [x] **BK-WBS-07.08.04** sole converted worker의 conditional retirement·거절·취소·회수/retry 경계를 검사한다. — 상태=D(검증완료); 선행=-; 근거=E49, 1463ccfa focused16/full platform387 PASS·ignored2 및 actual ALSA null1 PASS; 범위=software lifecycle, 물리/GUI는 별도.
  - [ ] **BK-WBS-07.08.05** retained Target controller의 open/start/poll/prime/commit을 새 coordinate authority·pause·입력 merger와 원자적으로 연결한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.01, BK-WBS-07.08.02, BK-WBS-07.08.03, BK-WBS-07.08.04; 필요 근거/다음=E51 controller18·실제 publication 할당0회1 및 E55 독립 review/CLI PASS; 실제 native desktop acceptance와 완료 기록 gate는 남음.
  - [ ] **BK-WBS-07.08.06** 새 retained Target 교체를 실제 solo/cohort 공통 pump에 연결하고 acknowledged pause·입력 drain·publication·resume을 검증한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.09.03; 필요 근거/다음=E54 실제 Owner history 경합 연결3 및 E55 독립 review/CLI PASS; 실제 GUI solo/local 시작은 장치 부재로 BLOCKED_ENV이며 native pause/Output/resume/capture와 완료 기록은 남음.
  - [ ] **BK-WBS-07.08.07** direct SourceFrame/SourceCounter retained 교체의 전체 controller·Owner/UI·공통 pump 및 history/pin 수명을 연결한다. — 상태=N(미구현); 선행=-; 필요 근거/다음=Target 기초를 Source 완료로 확장하지 않음; 원래 Source descriptor/query·callback retirement·cold history·WASAPI/ASIO/CoreAudio 경로와 실패/회수/재시도 검사.
  - [ ] **BK-WBS-07.08.08** 기존 scalar 교체와 finite-end ACK·held tail·취소/실패 경로가 새 retained 교체 변경 후에도 보존되는지 재검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=기존 구현/fixture는 존재하나 current 합쳐진 후보의 regression·실제 native crossing·end/drain/owner cleanup 및 독립 확인 필요.
  - [ ] **BK-WBS-07.08.09** Windows/macOS와 WASAPI↔ASIO 등 foreign/cross-backend retained 교체의 현재 연결·applied format·누락 경계를 감사한다. — 상태=U(최신현황감사필요); 선행=-; 필요 근거/다음=Target Linux/foreign stub typing을 전체 native 적용으로 간주하지 않음; 원래 backend/epoch/latency/history/cleanup 기준과 실제 SDK·host 실행 범위를 확인.
  - [ ] **BK-WBS-07.08.10** 실제 native retained 출력 교체에서 두 progressing 관측·입력 provenance·paused stale 보존·음향 alignment를 검증한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=실제 evdev/IOHID/Raw Input·native clock·출력 장치/ASIO SDK·물리 loopback; null/software/stub는 대체 근거가 아님.
- [ ] **BK-WBS-07.09** rate/buffer/channel/encoding 변경을 실제 설정 요청·capability·applied 결과까지 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E19 target rate/buffer/period/matrix UI bridge 검증; 실제 launcher/encoding 제어·pending PCM 해석 보존은 계속 필요.
  - [x] **BK-WBS-07.09.01** Linux encoding 설정·profile·CLI 및 실제 desktop Settings 편집/거절/Apply·재시작의 bounded software 경로를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E50, 18d9c414 독립 code/CLI QA·실제 Settings/profile·129-row 경계; 범위=Linux cold 설정, acknowledged-paused Output/물리는 별도.
  - [x] **BK-WBS-07.09.02** Linux encoding ALSA request/capability/applied·solo/local startup·recorded replay와 pending 해석 거절/recovery를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E50, 실제 production adapter/cold consumer·독립 app QA 및 두 opted ALSA null 검사; 범위=software encoding, retained Apply/물리/foreign는 별도.
  - [ ] **BK-WBS-07.09.03** 새 retained Target typed Output Owner/UI 요청·응답·capability refresh를 교체 controller에 연결한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.05; 필요 근거/다음=E54 경합 수정 및 E55 독립 review/CLI PASS, 실제 Settings Apply/reopen 확인; paused Output의 실제 applied capability·resume 및 완료 기록 gate는 남음.
  - [ ] **BK-WBS-07.09.04** 실제 acknowledged-paused Output GUI에서 retained 설정 Apply/거절·capability refresh·Back/reopen을 실행한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=E50 evdev 부재 BLOCKED_ENV; admitted native play/paused ACK 뒤 실제 입력과 current binary로 검사, Settings/null fixture 결과로 대체하지 않음.
  - [ ] **BK-WBS-07.09.05** 남은 모든 지원 플랫폼·backend의 rate/buffer/channel/encoding 요청·capability·실제 applied 및 pending PCM 해석 범위를 감사·연결한다. — 상태=U(최신현황감사필요); 선행=-; 필요 근거/다음=원래07.09 전체 기준·E19 target bridge와 미지원 선택의 명시 거절 유지; Source/foreign/실제 launcher·장치 조합을 확인하고 missing seam을 별도 bounded 단위로 갱신.
- [ ] **BK-WBS-07.10** startup/pause/resume/finite end ACK가 lookahead/held-release가 아니라 native crossing을 기다린다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E16 공개 audio pump의 pause/resume/held 순서 검증; 실제 launcher owner→worker→native 관측 E2E 및 finite end는 계속 필요.
- [ ] **BK-WBS-07.11** WASAPI↔ASIO 등 cross-backend 교체의 현재 소스 범위와 missing seam을 audit한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=WindowsRequest/공통 owner는 양쪽 open과 원래 Mixer·native clock 관측을 이미 지원; 현재 백엔드만 선택하던 Windows UI mapper와 output-only schema를 [대상 선택 계약](REQ__windows-output-backend-switch.md)에 따라 연결 중. 실제 SDK/장치 전환·latency/cleanup 검증은 남음.
- [ ] **BK-WBS-07.12** unknown/stale/wrong-domain 관측·취소·실패·재시도가 owner를 유실하지 않는다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=모든 실제 launcher/replacement 회귀.
- [ ] **BK-WBS-07.13** 모든 OS solo/local launchers가 실제 선택 class/gauge 정책 검증을 통해 network를 준비한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E1/E8 일부; 현재 Windows/macOS parser·foreign typing 재검사.
- [ ] **BK-WBS-07.14** 여러 buffer/backend/rate에서 시작·정지·재개·교체의 음향 alignment를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=물리 loopback; 소프트웨어 clock 통과와 별도.

### 08. BMS 파일·방언·판정·미디어

기능 연결: BK-022, BK-023, BK-024, BK-025, BK-026, BK-028, BK-029. 요구: REQ__bms-adapter / bms-preparation / judge-hazards.

- [x] **BK-WBS-08.01** 기본 BMS parser/분기 seed/채널/radix/원본 identity를 bounded adapter에서 처리한다. — 상태=D(검증완료); 선행=-; 근거=E0,E1.
- [x] **BK-WBS-08.02** UTF-8/BOM/Shift-JIS와 독립 raw/decoded 크기·경로·파일 budget을 처리한다. — 상태=D(검증완료); 선행=-; 근거=E1.
- [x] **BK-WBS-08.03** LNOBJ/LNTYPE·mine의 원래 stage/source/gauge 의미를 fixture로 검증한다. — 상태=D(검증완료); 선행=-; 근거=E0,E1.
- [x] **BK-WBS-08.04** 선택 gauge·명시 class mapping·EX 계산이 opaque grade를 추정하지 않는다. — 상태=D(검증완료); 선행=-; 근거=E1.
- [x] **BK-WBS-08.05** 연습 구간이 원래 TOTAL/note-count gauge context를 보존한다. — 상태=D(검증완료); 선행=-; 근거=E1.
- [x] **BK-WBS-08.06** RANK/DEFEXRANK metadata를 typed validation·명시 precedence로 처리한다. — 상태=D(검증완료); 선행=-; 근거=E30, [typed metadata 계약](REQ__bms-judge-rank.md); timing preset은08.07에 유지.
- [x] **BK-WBS-08.07** key/scratch/LN-end별 versioned timing preset을 기존 ClassifiedWindow에 연결한다. — 상태=D(검증완료); 선행=-; 근거=E41, [고정 numerical preset·staged core 계약](REQ__bms-timing-presets.md); ClassifiedWindow·completion·record identity·native/browser 선택 연결의 독립 code review/CLI96 및 현재 WASM의 실제 solo/history/replay/seek/settings/stop-retry/local capture PASS. 범위=opt-in software numerical windows 및 BeatKernel Hold v1; 물리 장치·전체 원본 엔진 호환 및08.08/08.09 정책은 별도. 기본값 변경 없음.
- [ ] **BK-WBS-08.08** 기본 timing dialect와 역사적 LR2/nanasi 호환 범위를 확정한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=원문 spec만으로 millisecond 표를 추정하지 않음.
- [ ] **BK-WBS-08.09** 동적 EXRANK/A0 의미와 필요한 timed policy를 primary source로 확인한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=서로 다른 engine 동작을 동일 표준으로 합치지 않음.
- [x] **BK-WBS-08.10** fresh unmatched/repeat/refused 입력을 구별하는 authoritative empty-POOR disposition을 만든다. — 상태=D(검증완료); 선행=-; 근거=E32,REQ__input-disposition; penalty/dialect 선택은08.11에 유지.
- [ ] **BK-WBS-08.11** empty-POOR의 후보/반복/게이지/combo/capture 의미를 선택 dialect에 고정한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=preroll 정책과 구별; replay/network identity에 포함.
- [x] **BK-WBS-08.12** WAV/FLAC/Vorbis/MP3가 실제 bounded preparation과 Mixer 출력까지 연결된다. — 상태=D(검증완료); 선행=-; 근거=E1; 포맷별 declared trim/limits.
- [x] **BK-WBS-08.13** same-format Ogg chaining의 BOS/EOS/serial/독립 trim/aggregate limit를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E1; malformed·mixed format 전체 거절.
- [x] **BK-WBS-08.14** 전 포맷의 잘린 파일·큰 metadata·late decode 실패·rate/channel 정책을 확장 검증한다. — 상태=D(검증완료); 선행=-; 근거=`255c400`의 독립 DEEP code review 후 CLI QA47/0/0ignored(새 corpus15·allocation5·기존 preparation27), Rust2021 format/diff/WBS 검사 exit0, [준비 corpus/측정 지침](../verification/GUIDE__audio-preparation-corpus.md)과 [검증 기록](../changes/2026-10-10-audio-preparation-corpus.md). 범위=네 포맷의 public decoder·memory/file preparation·후반 codec 오류·metadata·rate/channel·PCM/aggregate/alias·실제 sparse encoded-limit 및 명시된 TLS allocation-request corpus. 총 RAM sandbox/전체 codec conformance·실제 음향·Harness close 증거 아님.
- [ ] **BK-WBS-08.15** static BGA BMP/PNG/JPEG·layer/poor/opacity/crop/canvas·파일명 호환을 실제 화면에서 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=기존 순수 fixture + 실제 native/browser rendering.
- [ ] **BK-WBS-08.16** EXBMP RGB/color-key/근사 matching의 정확한 호환 규칙을 확정한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=원본 engine 문서/asset 사례·허용 오차.
- [x] **BK-WBS-08.17** video decode·timestamp/reorder/frame budget·off-thread 공급을 구현한다. — 상태=D(검증완료); 선행=-; 근거=E34,REQ__bms-video의 순수 계약과 실제 native/browser decode-to-Scene·GPU·플레이 경로; 지원 코덱/배포 선택은08.18에 유지.
- [ ] **BK-WBS-08.18** video codec/backend/redistribution 라이선스와 지원 범위를 선택한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=MIT/non-ASIO 배포 조건과 실제 third-party 감사.

### 09. 기록·리플레이·연습·저장 경쟁

기능 연결: BK-008, BK-031, BK-032, BK-033, BK-034, BK-035, BK-038, BK-039. 요구: plan §12; REQ__section-restart / bms-player.

- [x] **BK-WBS-09.01** 실시간과 replay가 동일 JudgeEngine 전이와 accepted-operation capture를 사용한다. — 상태=D(검증완료); 선행=-; 근거=E0,E1.
- [x] **BK-WBS-09.02** 반복 replay sequence/hash와 snapshot seek/reverse가 기준 재생과 일치한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-09.03** chart/rules/options/class/gauge/seed/section 불일치 기록을 거절한다. — 상태=D(검증완료); 선행=-; 근거=E1.
- [x] **BK-WBS-09.04** 자기/타인 저장 기록을 원래 prefix로 재구성하고 로컬 판정과 구분한다. — 상태=D(검증완료); 선행=-; 근거=E1.
- [x] **BK-WBS-09.05** 기록 catalog/원래 prefix와 실제 archived completion/score/comparison을 구분한다. — 상태=D(검증완료); 선행=-; 근거=E1.
- [x] **BK-WBS-09.06** host-independent RecordedSetup preview와 native facade의 실제 admission이 일치한다. — 상태=D(검증완료); 선행=-; 근거=E1: AC023 4개.
- [x] **BK-WBS-09.07** 읽기 전용 FrozenRecordPreview가 TimingRecord/history를 보존하며 live authority를 재구성하지 않는다. — 상태=D(검증완료); 선행=-; 근거=E1/E9: AC024 9개.
- [x] **BK-WBS-09.08** archive 오류에도 유효 prefix를 유지하고 잘못된 선택/path/history를 원자적으로 거절한다. — 상태=D(검증완료); 선행=-; 근거=E1.
- [x] **BK-WBS-09.09** 특정 구간 준비가 원래 note time과 overlap BGM suffix·PCM frame 선택을 유지한다. — 상태=D(검증완료); 선행=-; 근거=E0,E1.
- [ ] **BK-WBS-09.10** F5 pinned retry·북마크·구간 편집·finite loop가 실제 UI/native에서 같은 의미로 동작한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=논리 fixture와 실제 cleanup/restart 대조.
- [x] **BK-WBS-09.11** gapless loop/scrub에서 같은 출력 timeline을 유지하며 reopen gap을 제거한다. — 상태=D(검증완료); 선행=-; 근거=E35; 범위=실제 production owner/pump와 주입 native 관측을 연결한 software E2E; 물리 측정은09.12.
- [ ] **BK-WBS-09.12** 장시간/반복 구간 재시작의 실제 키음/BGM/노트 동기화를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=물리 출력/loopback; 논리 hash만으로 완료하지 않음.
- [ ] **BK-WBS-09.13** profile/replay/archive 저장의 interruption·동시 publish·symlink·capacity 오류를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 부분 근거=E37 개별 replay/archive 및 E38 profile의 실제 파일 I/O·오류 주입·동시 게시·공개 save/load; 필요 근거/다음=프로세스 interruption 후 재실행/recovery·실제 capacity 및 전체 원래 저장 안정성 기준.

### 10. UI·렌더러·선언형 컴포넌트·수명

기능 연결: BK-030, BK-032, BK-036, BK-037, BK-044, BK-045, BK-046, BK-047, BK-048, BK-049, BK-050. 요구: REQ__display-declarative-ui / browser-retained-menus; UI GUIDE.

- [x] **BK-WBS-10.01** winit/wgpu와 native/window 없는 graphics 모델의 feature 경계를 유지한다. — 상태=D(검증완료); 선행=-; 근거=E6,E9.
- [x] **BK-WBS-10.02** atoms→molecules→organisms→screens의 typed 선언형 구조를 유지한다. — 상태=D(검증완료); 선행=-; 근거=E1; 실제 공유 화면 선언.
- [x] **BK-WBS-10.03** MountedLayout가 stable NodeId·부분 relayout·paint/hit 공유 clip을 유지한다. — 상태=D(검증완료); 선행=-; 근거=E1.
- [x] **BK-WBS-10.04** 변경 없는 signal/geometry/페이지는 기존 packet/cache를 재사용한다. — 상태=D(검증완료); 선행=-; 근거=E1.
- [x] **BK-WBS-10.05** Selection 선언형 화면의 search/목록/선택/행 reflow를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E1.
- [x] **BK-WBS-10.06** Settings 선언형 화면의 draft/edit/pending/control ID와 부분 갱신을 검증한다. — 상태=D(검증완료); 선행=-; 근거=E1.
- [x] **BK-WBS-10.07** Display 선언형 화면의 설정·resize·clip·rejection을 검증한다. — 상태=D(검증완료); 선행=-; 근거=E1.
- [x] **BK-WBS-10.08** Records 선언형 화면의 prefix/history/grade/comparison 페이지와 cache를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E1.
- [x] **BK-WBS-10.09** Practice 선언형 화면의 정확한 구간 preview/컨트롤/resize를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E1: focused9 + combined.
- [x] **BK-WBS-10.10** Players 선언형 화면이 실제 native/browser roster·capability·할당 gate를 보존한다. — 상태=D(검증완료); 선행=-; 근거=E1: 7개; label truncation 원래 paint 계약.
- [x] **BK-WBS-10.11** Devices 선언형 화면이 source identity/page/selectable/capability를 보존한다. — 상태=D(검증완료); 선행=-; 근거=E1: 7개.
- [x] **BK-WBS-10.12** Results 선언형 화면이 genuine/frozen 결과와 원래 페이지·점수·순서를 보존한다. — 상태=D(검증완료); 선행=-; 근거=E1: 6개.
- [x] **BK-WBS-10.13** screen/fragment/back-stack의 suspend/resume/dispose와 취소를 명시적으로 관리한다. — 상태=D(검증완료); 선행=-; 근거=E0,E1; native/browser 전체 flow는 아래 추가.
- [x] **BK-WBS-10.14** 노트 visible-range/index/cache가 전체 chart의 매-frame 순회를 피한다. — 상태=D(검증완료); 선행=-; 근거=E0,E1.
- [ ] **BK-WBS-10.15** 비정상 dense 노트/큰 chart/장시간 projection에서 capacity와 정확한 표시 정책을 검증한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=GPU cache/explicit limit·성능 및 누락 검사.
- [ ] **BK-WBS-10.16** 개별 component fractional transform/opacity/easing을 paint/hit/clip에 공통 적용한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E17/E18 browser Selection/Display, E24 native Selection/Display 소유자, E25 실제 native software Vulkan 이동/역 hit; E40 [Practice 연결](../changes/2026-10-10-practice-component-motion.md)의 공통 API·browser/native 소유자·accepted-pose IME 구현, 독립 code/CLI/browser QA 및 실제 native window/direct PNG 확인. Desktop MCP 동일 창 연결은 BLOCKED_ENV이며 나머지 화면 연동·OS 입력/IME·하드웨어 검증은 필요.
- [ ] **BK-WBS-10.17** component animation scheduler의 frame budget·취소·suspend/resume/dispose를 구현한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E17 64-slot/수명/cache·worker scheduling, E18 browser 51프레임/dispose, E24 native 수명 fixture, E25 native 22프레임/cache/dispose; 실제 성능/frame budget 및 독립 QA 필요.
- [ ] **BK-WBS-10.18** font fallback/Unicode/IME/clipboard/selection이 실제 native/browser 입력 방법에서 동작한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=순수 editor fixture + 실제 API/글꼴.
- [ ] **BK-WBS-10.19** 0-size/resize/DPI/focus loss/renderer loss/close가 owner cleanup과 input geometry를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 winit/GPU/browser lifecycle.
- [ ] **BK-WBS-10.20** Selection→설정→로딩→플레이→pause/output→기록/결과→Back/재시작 전 흐름을 GUI로 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 shader/클릭/키/수명·독립 GUI QA.

### 11. 브라우저 WASM·Worker·오디오·입력

기능 연결: BK-040, BK-041, BK-042, BK-045, BK-047, BK-050, BK-051, BK-052, BK-053. 요구: REQ__bms-browser / browser-retained-menus.

- [x] **BK-WBS-11.01** 현재 source의 실제 WASM library/AudioWorklet/render package를 빌드한다. — 상태=D(검증완료); 선행=-; 근거=E39; pinned wasm-bindgen 0.2.129·native-free dependency 검사 및 실제 current package 실행 확인. 전체 browser/runtime acceptance는 별도.
- [ ] **BK-WBS-11.02** Gameplay Worker가 준비·판정·capture·network owner를 담당한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=현재 실제 bridge·stalled renderer 관찰.
- [ ] **BK-WBS-11.03** Renderer Worker/OffscreenCanvas가 GPU와 local retained views를 소유한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 current WASM renderer·Scene/메뉴 수명.
- [ ] **BK-WBS-11.04** 모든 메뉴의 business owner·상태 packet·semantic action을 실제 Renderer view에 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=Selection/Settings/Practice/Display/Records/Players/Devices/Results.
- [x] **BK-WBS-11.05** 실제 Records preview·own/other opponent·history를 read-only packet으로 연결한다. — 상태=D(검증완료); 선행=-; 근거=E42; AC024 모델과 actual retained browser navigation의 독립 감사 완료.
- [ ] **BK-WBS-11.06** bounded packet·generation/screen/revision 토큰이 stale/hostile action을 원자적으로 거절한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 generated WASM + Worker/RenderClient.
- [ ] **BK-WBS-11.07** Window에 렌더/준비/대규모 model 갱신을 되돌리지 않고 입력·필수 gesture bridge만 둔다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=main thread CPU trace 및 source audit.
- [ ] **BK-WBS-11.08** AudioWorklet이 실제 callback·output clock·buffer·finite drain을 처리한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 browser audio; mock clock만으로 완료하지 않음.
- [x] **BK-WBS-11.09** portable Worklet 입력/출력 report의 extent·gap·overflow·queue/start/end 산술을 검증한다. — 상태=D(검증완료); 선행=-; 근거=E1의 worklet_audio_fixtures.
- [ ] **BK-WBS-11.10** keyboard/touch/pointer/HID/gamepad가 원래 source·timestamp·binding을 유지한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 browser 입력과 지원 capability.
- [ ] **BK-WBS-11.11** 1인 자동/다인 source 선택·retired ID 미재사용·cancelled chooser를 실제 흐름에서 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=genuine roster import/export와 permission.
- [ ] **BK-WBS-11.12** IME/file/clipboard/HID/audio permission의 trusted gesture와 취소를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=비동기 Worker action이 권한 클릭을 합성하지 않음.
- [ ] **BK-WBS-11.13** SAB/COOP/COEP와 message fallback의 지원·실패 경계를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 배포 header·stall/backpressure.
- [ ] **BK-WBS-11.14** render/GPU stall과 resize가 gameplay/audio/입력 acquired-prefix에 영향을 주지 않는다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=의도적 stalled Renderer·submitted geometry·성능 trace.
- [ ] **BK-WBS-11.15** Worker 오류·lost renderer·audio failure·navigation close가 joined stop을 수행한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 owner retirement/cleanup.
- [ ] **BK-WBS-11.16** 지원 브라우저와 실제 touch/HID/gamepad에서 API/permission/clock 한계를 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=브라우저·장치 matrix; 미지원은 명시 refusal.

### 12. QUIC·WebTransport·멀티플레이

기능 연결: BK-038, BK-039, BK-040, BK-041, BK-042, BK-043. 요구: selected-policy-network; multiplayer/room contracts.

- [x] **BK-WBS-12.01** native QUIC의 실제 connect/accept/cleanup이 같은 Tokio runtime 문맥에서 동작한다. — 상태=D(검증완료); 선행=-; 근거=E11 실제 connect/accept·positive exchange·pending cancellation/joins; SIGABRT 회귀 수정.
- [x] **BK-WBS-12.02** 인증서 CA/server-name 오류와 identity 불일치를 실제 QUIC에서 거절한다. — 상태=D(검증완료); 선행=-; 근거=E12 실제 별도 CA/name/identity negative 사례와 no Connected/Ready/start 및 bounded join.
- [x] **BK-WBS-12.03** 실제 WebTransport native client와 HTTP/3 relay room을 상호 연결한다. — 상태=D(검증완료); 선행=-; 근거=E14 실제 HTTP/3 native4-case와 owned relay 종료; browser는 12.04 별도.
- [ ] **BK-WBS-12.04** 브라우저가 actual BrowserRoomOwner/WASM identity로 WebTransport room을 재생한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=[실제 room 플레이 계약](../runtime/REQ__browser-room-play.md)과 strict 2-host acceptance runner 준비. Private NSS CA의 HTTPS 수용/다른 CA 거절은 확인했으나 실제 QUIC certificate handshake가 거절되어 BLOCKED_ENV; eligible system-trusted HTTP/3 endpoint의 두 host 자연 완료·peer score·final ACK/drain 및 native/browser 상호운용 필요. Node47/syntax나 certificate bypass는 완료 근거가 아님.
- [ ] **BK-WBS-12.05** Origin/trust/path/limits·setup cancellation·pending handshake stop/join을 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 연결/UDP traffic·no false Connected/Ready.
- [x] **BK-WBS-12.06** 선택 class/gauge/profile/section/seed의 canonical network identity를 순수 검증한다. — 상태=D(검증완료); 선행=-; 근거=E1.
- [x] **BK-WBS-12.07** actual selected cohort owner가 원래 member별 policy header를 port로 전달한다. — 상태=D(검증완료); 선행=-; 근거=E8; 실제 QUIC worker 종료 포함.
- [ ] **BK-WBS-12.08** 선택 정책을 모든 native/browser solo/cohort 실제 네트워크 흐름에 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=기본 wire 의미 유지·capture disabled admission 포함.
- [ ] **BK-WBS-12.09** room roster·host seal·ready barrier·participant ID와 취소/탈퇴를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 2명 이상 cohort 및 3/4명 이상 room.
- [ ] **BK-WBS-12.10** 측정 software start/preroll/committed schedule가 같은 song target에 연결된다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=native/browser 실제 start; 물리 동시 소리는 별도.
- [ ] **BK-WBS-12.11** progress/results/final ACK/drain·disconnect가 로컬 판정과 원래 prefix를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=full end/finite end/반복 disconnect 실제 exchange.
- [ ] **BK-WBS-12.12** 연결/peer 입력/크기/속도 제한과 malformed protocol 처리·보안 경계를 audit한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=parser/fuzz + 실제 TLS/Origin/서비스 자원.
- [ ] **BK-WBS-12.13** 서로 다른 OS/browser의 실제 플레이·latency/clock/start 분포를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=다중 host/device; remote prefix를 로컬 시계로 사용 금지.

### 13. 결정론·오류 안정성·성능·물리 증거

기능 연결: BK-011, BK-018, BK-021, BK-035, BK-055, BK-056, BK-057. 요구: plan §§16–17, 24; runtime-benchmark / acceptance-evidence.

2026-10-10 입력 수집 종료 추가 개발 검증: 정상 worker 종료와 최종 FIFO 소진을
구분하여 마지막 이벤트/cut 및 startup 보관 입력의 조기 종료를 수정했다.
실제 collector/startup 그룹 18개와 실제 collector→solo/cohort pump→판정/기록을
연결한 오디오 그룹 41개가 통과했다. 1~4인, marker 용량·소스/정리 오류,
관측 시계 부족·stale·취소를 포함한다. 독립 DEEP 코드/권한 경계 검토와 CLI QA도
PASS이며, 전체 app 2257/0/6·native main 307/0/5 및 실제 CLI 7/7을 확인했다.
native 관측은 주입된 software 증거다. 13.04의 전체 장애 범위는 W로 유지한다.
근거와 재현 명령은 [검증 지침](../verification/GUIDE__runtime-fault-schedules.md)의
Native collector termination 절에 있다.

- [x] **BK-WBS-13.01** seeded property/corpus가 시간·codec·compiler·replay 기준 결과와 일치한다. — 상태=D(검증완료); 선행=-; 근거=E0; 장기 fuzz 완료와 구별.
- [ ] **BK-WBS-13.02** 모든 layer의 pure test port와 실제 adapter contract test를 coverage map으로 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=UI/business/native IO 의존성·경계 표.
- [ ] **BK-WBS-13.03** parser/compiler/input/replay/room codec에 지속 fuzz·corpus/minimization을 구축한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=REQ__layer-fuzz의 실제 sanitizer campaign·재현 artifact·minimization·오탐 방지 및 외부 지속 CI 실행.
- [ ] **BK-WBS-13.04** 수집·queue·clock·output·network·UI에 deterministic fault/chaos schedule을 구축한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=입력/시계/queue의 18개 fixture 및 Runtime→Mixer mono/stereo PCM 거절·원래 owner 회수·실제 consumer 해제의 13개 scenario(19개 pair) 독립 code review/CLI QA 통과, [검증 지침](../verification/GUIDE__runtime-fault-schedules.md). [연결된 player network/UI 계약](../runtime/REQ__player-fault-isolation.md)의 7개 새 테스트로 실제 Runtime prefix·client/relay wire/ACK/drain·portable room controller UI 장애를 검사하고 치명 즉시 UI reply 오류 누락을 재현·수정; 최종 app 전체 2172 PASS/0 FAIL/6 ignored, 독립 code review·CLI QA·문서 검토 PASS(QA는 신규 7 및 기존 관련 67개 재실행). 실제 native output/network/UI·allocation/short write/spawn/panic 동시 경계는 남음.
- [ ] **BK-WBS-13.05** 대규모 노트·64-member·오랜 플레이·반복 seek/rebind의 soak/stress를 실행한다. — 상태=W(구현·연동중); 선행=-; 근거/다음=E33/E43 dense judge/replay/projector·자식 CPU/RSS report와 E44 실제64-member retained practice/pause/refusal·별도 admitted rebind software 검증 완료; E45 전체20h/week sparse1000Hz 가상 timeline 독립 code/security/QA 완료; 전체 player CPU/memory/queue/owner leak·표준-rate/native·장시간 soak는 계속 필요.
- [ ] **BK-WBS-13.06** 전체 RT callback과 판정 hot path alloc/lock/I/O/error 경로를 계측한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=일반/오류/포화·모든 backend; scoped 테스트만 확대하지 않음.
- [ ] **BK-WBS-13.07** p50/p95/p99/max·drop/xrun·scheduler jitter 계측을 재현 workload에 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=계측 위치/provenance/용량·관측 불가 표시.
- [ ] **BK-WBS-13.08** 성능 기준 workload·CPU/RAM/GPU/배포 artifact/실행 절차를 고정한다. — 상태=W(구현·연동중); 선행=-; 근거/다음=E31/E43 fixed runtime/dense software matrix의 CPU/RSS/artifact report 검증 완료; 전체 player 대형 chart·dense UI stall·다인·GPU·장시간 비교는 계속 필요.
- [ ] **BK-WBS-13.09** 입력 1kHz 이상에서 device→collector→runtime 손실/순서/jitter를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=실제 source/device.
- [ ] **BK-WBS-13.10** buffer 크기별 native audio underrun/latency·교체 후 지연을 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=실제 WASAPI/ASIO/ALSA/CoreAudio.
- [ ] **BK-WBS-13.11** 입력→소리·구간 재시작의 end-to-end 음향 latency/drift를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=물리 loopback/측정장비와 raw data.
- [ ] **BK-WBS-13.12** 동일 조건에서 경쟁 플레이어와 CPU/memory/latency를 비교해 목표 성능을 검증한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=동일 workload/device/build; osu! 대비 우위 추정 금지.
- [ ] **BK-WBS-13.13** 안정성 목표의 coverage/soak/fuzz/결정론 한계와 재현 실패를 기록한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=SQLite급이라는 표현을 테스트 수/소스 존재로 보증하지 않음.

### 14. 빌드·독립 검증·문서·라이선스·출시

기능 연결: BK-054, BK-055, BK-056, BK-057, BK-058, BK-059. 요구: plan §§18–24; MIT/ASIO distribution.

- [x] **BK-WBS-14.01** 전체 WBS의 stable ID·완료 기준·근거·변경 이력과 자동 집계 기준을 구축한다. — 상태=D(검증완료); 선행=-; 근거=E10; 이후 구현자가 관련 leaf와 증거를 갱신한다.
- [ ] **BK-WBS-14.02** 현재 workspace/lib/bin/example/doctest의 debug/release 회귀를 실행한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=2011 lib만 전체 workspace 결과로 부르지 않음.
- [ ] **BK-WBS-14.03** Windows/macOS foreign typing과 실제 native build를 구분해 검사한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=C/archive stub은 SDK/device 실행 증거 아님.
- [ ] **BK-WBS-14.04** WASM/headless/desktop/선택 feature별 현재 build와 shader를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 pkg 재생성·GPU pipeline.
- [ ] **BK-WBS-14.05** fmt/clippy/CI의 current 회귀와 기존 실패를 구분하고 정식 matrix를 통과한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=hosted CI·native runner; user mise.toml 변경 금지.
- [ ] **BK-WBS-14.06** 공개 API/examples/rustdoc와 실제 실행·오류·설정/재시작 문서를 완성한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=원본 phase0–14/일반화 fixture usage.
- [ ] **BK-WBS-14.07** 전체 변경에 독립 code/security/document review를 완료한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=actual structural PASS; 현재 task PENDING.
- [ ] **BK-WBS-14.08** review PASS 뒤 독립 CLI/browser/desktop QA를 실제 환경에서 완료한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=qa-browser 필수; start receipt≠PASS.
- [x] **BK-WBS-14.09** MIT source·ASIO 제외 MIT/포함 GPLv3 정책을 문서·root license에 기록한다. — 상태=D(검증완료); 선행=-; 근거=E7.
- [ ] **BK-WBS-14.10** 실제 non-ASIO release의 third-party notice/feature/dependency/artifact를 감사한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=MIT 조건·코덱/폰트/GPU 의존성.
- [ ] **BK-WBS-14.11** ASIO combined release의 SDK notice/GPLv3/Corresponding Source를 감사한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 MSVC binary·재현 source/build bundle.
- [ ] **BK-WBS-14.12** 지원 OS/browser의 설치·기본 시작·실제 입력/오디오·upgrade/failure smoke를 완료한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=플랫폼 release matrix.
- [ ] **BK-WBS-14.13** 원본 plan §24와 모든 active WBS를 증거별로 확인한 뒤 최종 release를 만든다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=미완료/조건부를 숨기지 않는 Goal completion audit.

### C01. 조건부·미활성 요구사항 — 현재 분모 제외

기능 연결: BK-060. 요구: REQ__sdk-status 및 primary-goals.

- [ ] **BK-WBS-C01.01** 실제 C/C# host 요구가 확인되면 최소 create/input/update/destroy ABI/SDK를 구현한다. — 상태=C(조건부·미활성); 선행=-; 필요 근거/다음=host 요구 미확정; Rust API가 SDK 구현 완료를 뜻하지 않음.
- [ ] **BK-WBS-C01.02** 새 native 출력 API/device 계약이 확정되면 같은 Port/Adapter로 추가한다. — 상태=C(조건부·미활성); 선행=-; 필요 근거/다음=추가 API 범위를 확정한 뒤 capability/실제 driver 검사.
- [ ] **BK-WBS-C01.03** 계정·공식 순위·인증된 결과·anti-cheat가 요구되면 authority/정책을 별도 설계한다. — 상태=C(조건부·미활성); 선행=-; 필요 근거/다음=현재 casual/unauthenticated 경쟁 범위를 몰래 ranked로 바꾸지 않음.
- [ ] **BK-WBS-C01.04** pitch 유지 배속이 실제 요구되면 별도 time-stretch DSP를 audio port에 추가한다. — 상태=C(조건부·미활성); 선행=-; 필요 근거/다음=plan §11.4의 명시적 비-MVP 경계; pitch 변화 허용 read-head 배속과 다른 기능이며 현재 필수 완료로 활성화하지 않음.

## 실제 작업 순서와 병렬 경계

1. 독립적인 source/test 파일이 준비되면 병렬로 진행한다. 전체 요구를 오디오 뒤에
   직렬 대기시키지 않는다. 같은 파일/common contract와 dependent consumer는 순서를 둔다.
2. core target duration/converted-state → target pause/end planned basis → controller
   lifecycle/basis → ALSA adapter → actual solo/local/replay → live rate/buffer controls
   순서로 연결한다. source pull을 target clock으로 변장시키지 않는다.
3. mounted layout와 native/browser projection은 준비됐다. 화면 migration 검사를
   메뉴 owner·Worker navigation에 연결한 뒤 component motion과 실제 GUI QA를 진행한다.
4. QUIC runtime-context 수정은 real TLS loopback 재실행으로 확인한다. 그 다음 owned
   HTTP/3 relay/native/browser room/Origin/trust/final ACK를 실행한다.
5. RANK/empty-POOR/EXBMP/video의 primary-source 조사와 정책은 별도 진행한다.
   미응답 pre-play 정책을 대신 결정하지 않는다. 알려진 opt-in dialect의 준비와
   실제 새 기본 정책 채택을 구분한다.
6. hardware 접근이 필요한 E 항목은 장비/OS 조건을 보존한다. 환경이 없다는 이유로
   소프트웨어·다른 기능 개발을 멈추거나 해당 항목을 완료로 바꾸지 않는다.
7. compiler가 읽을 source/test writer를 먼저 STOP한다. root가 공통 registry·통합·진행
   상태를 관리하며 구현자와 test author의 파일을 분리한다. 동시에 하나의 compiler
   owner만 둔다. configured fanout8과 실제 agent 슬롯을 확인한다.
8. 마지막으로 frozen combined diff에 독립 review PASS → CLI/browser/desktop QA PASS →
   receipt-backed verify/close 순서를 따른다. 현재 broad task는 아직 PENDING이다.
   이 문서 작성이나 개발 검증 통과가 task/Goal close를 허가하지 않는다.

## 과거 병렬 task와 연결

과거 `TASK__parallel-player-requirements`의 **24개 AC / 17개 개발 milestone**
범위 매핑을 보존한다. 아래 표는 역사적 범위 위치이며 현재 focused task나 완료
상태를 뜻하지 않는다. 전체 분모 또는 별도 추가24개 작업으로 합산하지 않는다.

| AC | 실제 WBS 범위 |
| --- | --- |
| 001 | 01.10, 14.01 |
| 002 | 10.02–10.04 |
| 003 | 11.02–11.07, 11.11–11.15 |
| 004 | 08.13 |
| 005 | 02.05, 05.04 |
| 006 | 05.06–05.09, 07.04–07.09 |
| 007 | 07.02–07.10 |
| 008 | 12.06–12.08 |
| 009 | 12.01–12.05, 12.09–12.11 |
| 010–016 | 10.05–10.12의 Selection/Settings/Records/Practice/Players/Devices/Results |
| 017 | 07.04–07.13, 12.08, native/GUI registry 통합 |
| 018 | 10.16–10.17 |
| 019 | 14.01 및 전체 WBS |
| 020 | 14.02–14.08 |
| 021 | 10.10–10.11의 browser projection 기반; 03.11 E2E는 별도 |
| 022 | 05.09, 07.10 |
| 023 | 09.06 |
| 024 | 09.07–09.08, 11.05 |

위 범위 표는 scope 위치를 뜻한다. 묶인 행이나 range를 완료 개수로 세지 않는다.

## 현재 진행 작업과 연결

현재 focused task는 `TASK__retained-practice-output-replacement`이며 같은 run과
미완료 범위를 보존해 재개했다. `TASK__wbs-ongoing-work-breakdown`의 도구·문서는
03435bf4로 커밋하고 실제 독립 review/CLI QA를 통과했다. 이 추적 작업은 자동
검증 기록 누락 때문에 종료만 parked 상태이며 제품 기능 완료 수로 합산하지 않는다.

| retained task AC | 안정적인 실행 단위 | 현재 근거/다음 gate |
| --- | --- | --- |
| AC001 | BK-WBS-07.08.01 | E46 범위 한정 D; 기존 input31 회귀를 새 테스트로 세지 않음 |
| AC002 | BK-WBS-07.08.02 | E47 성공 green67 D; 앞선 실패 final 로그와 구분 |
| AC003 | BK-WBS-07.08.03 | E48 pause75·포맷 후 집중13 D; 실제 pump는 별도 |
| AC008 | BK-WBS-07.08.04 | E49 focused16/platform387/null1 D; 실제 hardware/GUI는 별도 |
| AC004 | BK-WBS-07.08.05 | E51 V; controller18·할당 검사1·관련 회귀 통과, 독립 gate 남음 |
| AC005 | BK-WBS-07.09.03 | E54 V; 경합 실패 재현 뒤 요청/관측25·pure app2682·할당1 PASS, 재리뷰/QA 필요 |
| AC006 | BK-WBS-07.08.06 | E54 V; 실제 Owner history port와 경합 연결3·pure app2682 PASS, 독립 gate 남음 |
| AC007 | 관련 child 전체의 검증 gate | 독립 review/QA·전체 통합 검증이며 별도 기능 leaf나 중복 완료 수를 만들지 않음 |

07.08.07/07.08.08/07.08.09/07.08.10과07.09.04/07.09.05는 원래 부모의
Source/scalar/finite/foreign/실제 GUI/물리/applied 전체 범위를 보존하는 남은 단위다.
구현자는 실제 소유 파일·수명·연동 경계에 맞춘 기존 child의 상태·근거·다음 gate를
구현/검증 milestone마다 갱신한다. 새 독립 범위가 확인될 때만 ID를 추가하고 분모
변화를 이력에 적는다. task AC/test 수를 제품 완료 수로 옮기지 않는다.

## 원본 명세 coverage

| 원본 범위 | 추적 위치 |
| --- | --- |
| §§1–4 제품·불변식·workspace·dependency | 01, 14 |
| §5 시간/clock/Transport | 02, 07 |
| §6 입력/normalizer/device adapter/binding | 03, 06, 11 |
| §§7–9 chart/interaction/judge | 04, 08 |
| §11 audio/RT/scheduling·§11.4 배속/sample reverse 정책 | 05, 06, 07, 13; 05.13–05.14; 조건부 C01.04 |
| §10 visual projection/외부 renderer | 04, 10, 11 |
| §13 일반화 및 §22 여섯 fixture 유형 | 03, 04.04–04.09, 04.12(각 여섯 조합의 결과), 13 |
| §12 replay/snapshot/seek/reverse | 02, 09 |
| §14 공개 API/composition root/platform/adapter 확장 | 01, 03–07, 14 |
| §15 threading/owner 경계 | 01, 03, 07, 11 |
| §16 performance와 §17 전체 테스트·장비 측정 | 13, 06.12–06.14 |
| §18 Phase0–14 완료 조건 | 01–14 전체 |
| §18 Phase15 conditional SDK | C01.01 |
| §§19–21 실행 규칙/반패턴/초기 MVP | 01, 13, 14; 초기 MVP로 현재 전체 범위를 축소하지 않음 |
| §23 초기 file scaffold, §24 최종 완료 조건 | 01, 03–07, 09–10, 13–14 |
| 이후 사용자 승인 BMS app/UI/browser/competition | 08–12 및 14 |
| 원래 59개 active feature + 조건부 BK060 | 각 절의 기능 연결; 검사기가 빠진 ID를 거절 |

새 세부 requirement가 발견되면 해당 leaf의 bounded criteria로 흡수할 수 있는지
확인하고, 독립적으로 검증해야 하는 작업이면 새 ID를 추가한다. feature 인벤토리는
역사적 source survey로 보존하고 **이 WBS가 전체 작업/진행률의 기본 진입점**이 된다.
상세 behavior/architecture/라이선스는 연결된 기존 REQ/GUIDE/ADR이 계속 소유한다.

## 변경 이력

- 2026-10-14 독립 QA: E55의 실제 재리뷰/CLI PASS 및 interactive desktop
  BLOCKED_ENV를 기록했다. 전체 완료 수는 유지하며 native 장치 acceptance와 자동
  완료 기록 부재를 software PASS로 덮지 않는다. 전체 player Goal은 계속 활성이다.

- 2026-10-14 리뷰 수정 재검사: E54의 수정 전 실패1과 수정 후25/3·전체2682·
  할당1 PASS를 확인해07.09.03/07.08.06을 W에서 V로 갱신했다. 전체 완료 수는
  유지하며 재리뷰/QA를 개발 검사로 대체하지 않는다.

- 2026-10-14 독립 리뷰: `015256a3`의 DEEP code review는 native authorization과
  history-retirement 요청 사이 경합1건으로 FAIL했다. 연결 fixture의 기본 no-op
  history port도 확인했다.07.09.03/07.08.06을 V에서 W로 되돌려 수정·회귀 레인을
  배정했다. 완료97/206·원래91/193은 유지하며 개발 PASS를 리뷰 PASS로 대체하지 않는다.

- 2026-10-14 공통 pump 검사: 실제 solo·2인 cohort·전환상태3개와 pure app2674,
  할당 회귀1을 통과해07.08.06을 W에서 V로 갱신했다. 말단 구현됨은163개이며
  완료97/206·원래91/193은 유지한다. desktop app3129/각 bin/check도 통과했으며
  독립 gate와 실제 GUI/물리 근거는 별도다.

- 2026-10-14 공통 pump 착수: AC005의 실제 개발 검증 뒤 AC006 구현·테스트 작성
  레인을 배정해07.08.06을 N에서 W로 갱신했다. 완료 수와 구현됨 수는 유지한다.

- 2026-10-14 Owner/UI 검사: AC005의 새16개와 기존1개, 관련 회귀와 모든 bin
  check를 실제 실행해07.09.03을 W에서 V로 갱신했다. 말단 구현됨(D+V)은162개,
  완료97/206·원래91/193은 유지하며 실제 공통 pump는 N이다.

- 2026-10-14 Owner/UI 착수: AC005 구현·테스트 작성 레인을 배정하고07.09.03을
  N에서 W로 갱신했다. 실제 검사 전이며 controller V·공통 pump N과 완료 수는 유지한다.

- 2026-10-14 controller: borrowed commit으로 prepared metadata 복제를 제거하고
  실제18 unit+1외부 할당 검사와 관련 회귀를 확인했다. controller V는 최종 독립
  gate까지 유지하며 Owner/UI·pump N 및 전체 집계97/206·91/193은 바꾸지 않는다.

- 2026-10-14 재개: AC004 수정 fixture의 실제18 PASS/실패0을 E51에 기록하고
  다음 allocation/회귀/독립 gate를 갱신했다. retained task는 같은 run에서 active로
  재개했다. controller V와 후속 Owner/UI·pump N, 확장97/206·원래91/193은 유지한다.

- 2026-10-14: 진행 중인 retained 출력 교체를 추적하도록 원래07.08/07.09의
  ID·제목·완료 기준·선행을 그대로 두고10개와5개 child로 분해했다. 원래 활성
  요구193개·D91개·조건부4개와60개 기능 coverage는 유지한다. 두 구조 부모 대신
  15개 말단을 집계하므로 확장 말단 분모는13개 늘어난다. 여섯 범위 한정 D는
  기존 committed 기초/encoding 검사를 가시화한 것이며 이번 WBS 작업의 새 제품
  구현 성과가 아니다. 실제 변경 후 집계·독립 검사 결과는
  [이번 변경 기록](../changes/2026-10-14-wbs-ongoing-work-breakdown.md)에 기록한다.
  AC004는 마지막 실제15 PASS/3 FAIL과 수정 후 미실행을 명시해 V,
  새 retained AC005/006은 N으로 남겼다. Source/scalar/finite/foreign/실제 GUI/물리
  기준과 현재 parked task의 review/QA gate를 없애지 않았다.

- 2026-10-11: genuine Records preview/history와 retained Own/Other controls의
  실제 연결을 독립 code review·CLI/browser QA·UX 및 원래 기준 감사로 확인해
  E42를 추가하고 11.05를 D로 갱신했다. 완료90→91, 활성193 유지.
  physical/전체 UI/출시 및 Harness attestation은 남은 기준에서 유지한다.

- 2026-10-10: 기존 판정 프리셋 code review/CLI96과 현재 실제 browser의
  남은 history/replay/seek/retry/local 검증 PASS를 대조해08.07을 D로 갱신했다.
  완료88→89, 활성193 유지. 프리셋 software 연결과 Harness attestation/close,
  전체 원본 엔진 호환·물리 검증을 구별한다.

- 2026-10-09: 영상 BGA의 native/browser 실제 디코딩·GPU·플레이 경로와
  독립 code review/QA를 확인하여 08.17을 D로 갱신했다. 완료84→85,
  활성193 유지. 물리 오디오·다른 OS 및 08.18의 배포/코덱 선택은 완료로
  확대하지 않는다.

- 2026-10-08: 사용자 요청에 따라 15개 영역으로 전체 목표를 분해했다. 초기 활성
  말단190개, 조건부3개; source-only/physical proof를 구별하고 문서 작성 시 확인한
  근거만 D로 기록했다. 기존 59개 feature를 제거하지 않고 모든 번호를 연결했다.
  정확한 최신 완료/대기 개수는 `tools/wbs_status.py`가 본문에서 산출한다.
- 2026-10-08: 독립 문서 scope audit에서 원본 §10/§12/§14 연결을 바로잡고,
  여섯 일반화 fixture의 별도 검증과 audio playback speed/reverse 두 검증을 추가했다.
  활성 분모190→193, 조건부3→4. 추가 항목은 근거 재확인 전 V/C이며 완료 수를 늘리지 않았다.
- 2026-10-08: 위 누락 수정 후 독립 bounded 문서 검토와 집계/ID/coverage 검사가
  통과하여 WBS 구축 14.01을 D로 갱신했다. 완료75→76, 활성 분모193은 유지했다.
- 2026-10-08: actual converted owner의 held/active provenance 회귀가 통과해
  05.07을 D로 갱신했다. 완료76→77, 활성 분모193 유지; target rebind/실제 native
  교체 완료는 이 부분 검사로 대신하지 않는다.
- 2026-10-08: 실제 QUIC 4-case 수정 회귀가 통과하여 12.01을 D로 갱신했다.
  완료77→78, 활성193 유지. 12.02의 원래 CA/name/identity 전체 기준을 유지하며,
  아직 미실행인 unrelated-CA native 사례 때문에 V를 유지한다.
- 2026-10-08: 원래 12.02의 전체 CA/name/identity 기준을 실제 5-case QUIC 회귀로
  확인해 D로 변경했다. 완료78→79, 활성193 유지; WebTransport/Origin/교차 host는 별도다.
- 2026-10-08: mixed-rate rebind/helper 회귀와 실제 native HTTP/3 room 교환을 확인해
  07.03/12.03을 D로 갱신했다. 완료79→81, 활성193 유지. 전체 controller/launchers와
  actual browser/WebTransport/GUI/hardware는 해당 남은 leaf를 계속 유지한다.
- 2026-10-10: 실제 retained-owner Desktop/pump/factory/converter 경로의 독립
  code/security/CLI QA와 양 OS all-targets typing이 통과해09.11을 D로 갱신했다.
  완료85→86, 활성193 유지.09.12 물리 동기화·browser parity·출력 교체와
  전체 제품/출시 검증은 해당 남은 leaf에서 유지한다.
- 2026-10-10: 원본 §22의 여섯 조합을 실제 Runtime/기록/replay에 연결하고 독립
  DEEP code review·core498/0·release38/38·CLI12/12 QA로 검증해04.12를 D로 갱신했다.
  완료86→87, 활성193 유지. 실제 게임 호환·물리 입력·native/browser 및 출시 조건은
  남은 leaf에서 유지한다.
- 2026-10-10: 개별 native replay/archive의 완전한 게시와 원본 오류 보존을
  실제 파일/동시 저장/public API/WASM 회귀로 검증해 E37을 추가했다.
  profile과 interruption/recovery 등 전체09.13 기준은 남아 V를 유지한다.
  완료87/활성193 및 전체 제품 범위는 변경하지 않았다.
- 2026-10-10: profile의 순수 codec/native IO 경계와 공유 staged publisher를
  연결하고 실제 파일·교체·동시 absent 분류·원본 오류/public API 검사를
  독립 DEEP code/security 및 CLI 명령 실행으로 확인해 E38을 추가했다.
  interruption/recovery 등 전체09.13은 V, 완료87/활성193 및 전체 제품 범위를 유지한다.

- 2026-10-10: 실제 current WASM main/audio 빌드와 pinned binder, 독립 native-free
  graph 확인 및 current connected browser 실행 근거 E39로11.01을 D로 갱신했다.
  완료87→88, 활성193 유지. 다른 browser/runtime/hardware 및 전체 출시 조건은
  이 범위의 PASS로 완료 처리하지 않는다.
