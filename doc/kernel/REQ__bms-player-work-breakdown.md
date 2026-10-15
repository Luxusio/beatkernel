# BMS 플레이어 전체 작업 분해 구조(WBS)

기준일: 2026-10-15. 목표: **범용 BeatKernel 기반의 고성능 cross-platform BMS 플레이어**.
사용자가 요청한 전체 TODO/진행률의 기준 문서다. 현재 병렬 개발 묶음만으로
전체 목표를 대체하지 않는다. [원래 명세](../../plan.md),
[플레이어 요구사항](REQ__bms-player.md),
[전체 완료 조건](REQ__plan-acceptance-evidence.md),
[기능 ID 인벤토리](REQ__bms-player-progress.md)의 활성 요구를 유지한다.
원래 명세의 초기 비목표는 이후 사용자가 승인한 UI/browser/멀티플레이 요구를
제외하는 근거가 아니다. 검증 연기는 2026-10-06 해제됐다.

2026-10-15 ASIO .14.01–.06은 V, .14.07은 W다. 이전 독립 review의3건을 a56e039b·8ee68fb1·272b564f에서 수정했고 최종 DEEP code/security 검토와 단일 독립 소프트웨어 QA가 실제 PASS했다. QA는 core749/0, platform lib507/0, featureful app lib3319/0·bins670/0, Source allocation1/0·CLI19/19와 foreign typing/C++ 검사를 실행했다. native21·adapter17·Windows composition1은 typed-only이며 실제 SDK/driver .14.08은 E다. 완료 표시는 hook-owned 최종 gate를 따르고 전체136/489(27.81%), 남음353·원래91/193을 유지한다. [진행 기록](../changes/2026-10-15-asio-retained-source-output.md)을 따른다.

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

## 2026-10-14 세부 작업 기준선

진척을 더 자주 확인할 수 있도록 기존 **미완료 말단117개를383개 실행 단위**로
분해했다. 기존 완료106개와 조건부4개는 그대로 유지했다. ID/제목/상태/선행을
포함한 기존 행을 모두 보존하고, 실제 구현·소비자 연결·소프트웨어 검증·실기기
증거의 경계에 하위 ID를 추가했다. 모든 항목을 일괄적인 구현/테스트 삼등분으로
늘리지 않고 각 범위의 구체적인 산출물과 다음 검사를 적었다.

새 말단 기준선은 **135/489(27.61%), 남음354개**다. 이 중29개는 기존 실제 검사
근거가 있는 하위 범위를 이번에 드러낸 것이며 새 기능29개를 개발했다는 뜻이
아니다. 원래 제품 요구사항은 **91/193(47.15%)**로 변하지 않는다. 분해 전의
106/223(47.53%)과 새 비율은 분모가 달라 직접적인 증감 비교에 사용하지 않는다.
이 숫자는 이번 재분해 시점의 기록이며 이후 현재값은 집계 명령으로 확인한다.

진척 갱신은 해당 하위 ID에서 한다. 구현·연동 중은 W, 구현 후 필요한 검사를
기다리면 V, 그 문장에 필요한 실제 검사를 통과하고 근거를 남기면 D로 갱신한다.
아직 시작하지 않은 결합 구현·리뷰/QA 작업은 N으로 둔다. 장비/정책/현황 확인이
필요한 범위는 E/P/U로 계속 남긴다. 소프트웨어 D가 생겨도 상위 native·물리·출시
요구 전체가 자동 완료되지는 않는다. 분해 전 기록은 아래 근거 인덱스와 변경
이력에 당시 수치로 보존한다.

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

E56: Source 후속 child의 독립 계획 검토는 실제 WASAPI가 SourceFrame이 아닌
원래 Counter position/frequency를 사용함을 확인했다. 기존 exact authority와
NativeOutputState를 재사용하고 Legacy/complete recovery·실제 worker step·불확실한
native admission의 retry 격리·등록/형별 검증 경계를 확정했다. 새 Source REQ와
기존 WASAPI recovery 계약을 먼저 기록했다. 이는 구현 PASS가 아닌 계획/착수 근거다.
아래 기존07.08.07을15개 실제 실행 경계로 나눠 Source 기초4개를 W, 나머지11개를
N으로 추적한다. 분모는206에서220으로14개 늘고 완료97은 유지된다. 원래 상위
요구 집계91/193과 모든 Source/native/browser/성능/출시 범위는 변하지 않는다.

E57: Source 기초를 기능별 `f4c00398`(state/history), `9e11757d`(port/remix),
`835cc813`(native epoch/pause)로 커밋했다. 실제 portable app lib는2711 PASS/
실패0·ignored6이며 Source port/remix10·native14·pause5 및 exact authority 회귀를
포함한다. platform state56·Source history30과 browser-only WASM check도 통과했다.
새 portable WASAPI production step은14 PASS, 별도 System allocator binary는
실제 alloc/realloc/dealloc 보정 후 success/refusal/authorized/release-failure/
quarantine retry 경로의 heap 호출0회로1 PASS다. 세부 필터는 전체 suite에 더하지
않는다. 초기 fixture의 byte 예산·지원 PCM Hz·constructor 경계·mut 설정 오류는
실패 로그를 보존하고 수정 후 재실행했다. Source 기초5개는 V이며 Windows COM
연결과 controller는 W다. 독립 최종 review/QA·Windows 실제 실행·전체 Source
적용은 아직 증명되지 않았다. 재현과 한계는
[Source 변경 기록](../changes/2026-10-14-retained-source-output-replacement.md)을 따른다.

E58: `1d19c952`의 complete WASAPI native ownership/복합 오류/cold history와
`72ac7c98`의 공유 lifecycle·Source controller를 커밋했다. 실제 portable app는
2732 PASS/실패0·ignored6, platform은411 PASS/실패0·ignored2다. Source controller21과
새 cold history3은 해당 suite에 포함된다. Source 실제 held publication과 기존
Target publication의 외부 할당 검사 각각1 PASS, native 연결 후 실제 WASAPI worker
할당 회귀1 PASS다. 새 complete native와 app adapter/Owner의 cached Windows Rust
typing은 exit0이나 C-stub을 사용하며 Windows COM/장치 실행은 아니다. native/controller
2개를 V로, 작성된 Owner/adapter2개를 W로 옮겨 Source7V·2W·6N이다. 실제 Owner
typed 오류와 authorization 경합을 추가 검사 중이며 UI/consumer/pump는 남는다.
완료97/220과 원래91/193은 유지하며 독립 최종 review/QA 근거로 확장하지 않는다.

E59: `4a4d7287`의 Source Owner와 canonical complete WASAPI adapter를 연결했다.
typed packet 실패 뒤 authorization이 오면 오류가 숨겨지는 실제 RED1건을 확인하고,
Source 포트의 static default-false IO classifier/Remix 전달로 raw IO만 재probe하도록
수정했다. Owner24·canonical packet11·controller21을 포함한 전체 portable app는
2761 PASS/실패0·ignored6, Source/Target 실제 publication 할당은 각각1 PASS다.
Windows app lib/tests typing은 exit0이며 새 canonical native/classifier5개는 형 검사만
됐다. typed validation·history·quarantine·복합 cleanup 오류는 그대로 보존한다.
Owner/adapter2개를 V로, 시작한 typed UI를 W로 옮겨 Source9V·1W·5N이며 실제
Windows consumer/pump·독립 최종 review/QA·native 실행은 남는다. 완료 수는 유지한다.

E60: `0b64b2ee`의 typed Source UI는 실제 Owner/controller 경유8 PASS, 전체
portable app2769 PASS/실패0·ignored6이다. capability를 borrowed commit 전에
검증하고 원래 요청/응답 ID·WouldBlock·cold 거절과 Ready hold를 보존한다. 최초
fixture의 optional matrix 변환 컴파일 오류는 수정 후 실제 재실행했다. UI를 V로,
Windows 실제 consumer와 공통 solo/cohort 연결을 W로 옮겨 Source10V·2W·3N이다.
두 레인은 독립 파일에서 병렬 작성하며 최종 combined 검증은 둘 다 끝난 후다.
공통 mutable query hook은 기존 읽기 전용 API를 기본 전달하고 조건부 Source 조회를
실제 Owner에 연결한다. Source 전체·독립 최종 gate·Windows 장치 완료를 선언하지 않는다.

E61: 실제 Source Frame/Counter × solo/cohort4개 연결 시나리오는 처음에 잘못된
fixture bootstrap을 거절했다. 원래 creation5는 유지하고 실제 Frozen seed 뒤
startup7을 배치해 초기 관측/transport를 바로잡은 뒤, 실제 production guard가
서로 다른 stream origin과 채택된 startup playback origin을 거절하는 결함을 확인했다.
Windows solo/local 실제 startup과 공통 정책도 두 위치의 차이를 허용한다.
Source controller를 V→W로 되돌리고 Source pause의 실제 start/current basis에서
정확한 이전 playback origin을 유도하는 수정을 작성했다. 1ns tamper 거절을
추가했으며 현재 실제 전체 회귀를 재실행 중이다. guard를 제거하거나 fixture의
시작점을 같게 만들어 숨기지 않았다. 새 Windows source/bin69와 전용 tests 형 검사는
통과했지만 이 실제 연결 실패를 PASS로 대체하지 않는다. Target의 유사 startup
가정은 별도 감사/회귀 대상으로 남기며 이번 Source 수정으로 완료하지 않는다.

E62: `bbdfea4b`는 Source startup origin을 실제 pause/start/current basis에서
정확히 계산한다. `e3f056d3`는 Windows shared complete owner와 공통 mutable query를
연결하고 `8cf352ca`는 실제 Frame/Counter × solo/cohort4개 시나리오를 검사한다.
실제 연결4 PASS, 전체 portable app2773 PASS/실패0·ignored6, Windows bin69 PASS다.
서로 다른 creation5/start7, 반복 교체·다른 Counter frequency·matrix·partial tail·
거절·WouldBlock·history/관측/query 승인 경합과 resume/loop/scrub/종료를 통과했다.
원래 Runtime/member/attempt·입력 capture codec/hash를 유지하며 실제 제출 PCM과
pending suffix의 전체 frame 수를 생성 cursor와 정확히 비교한다. 종료 active silence도
실제 마지막 render report/PCM에서 검증하며 임의 오차를 더하지 않는다.
Source/Target publication 외부 할당 각1 PASS와 Windows lib/tests typing도 통과했다.
controller/consumer/pump3개를 W→V로 복구해 Source12V·3N이다. 전체 D97·V78·W20·
N3·E14·P5·U3, 활성220·완료97과 원래91/193은 유지한다. 독립 최종 리뷰/QA 및 실제
Windows native 실행은 아직 완료하지 않았다. Target startup 감사는 별도 후속이다.

E63: frozen a7869f70의 실제 독립 DEEP 리뷰는 FIX_NOW3 FAIL을 반환했다.
양수 oversized Duration의 UI cold 거절 누락, Exact fractional Duration의 retirement
이전 검증 누락, pre-run MMCSS teardown 오류 유실이다. worker/native/Windows
consumer3개를 V→W로 되돌리고 공유 sizing 검증·실제 정리 경계와 독립 회귀를 수정한다.
Source9V·3W·3N, 전체 D97·V75·W23·N3·E14·P5·U3이며 분모/완료 수는 유지한다.
기존 연결2773 PASS를 이 새 실패의 완료 근거로 쓰지 않는다. 수정 후 두 번째/final
발견 배치와 새 정식 리뷰, 그 뒤 CLI/native QA 순서를 따른다.

E64: 정식 리뷰3건을 ef72c8d2(platform sizing/MMCSS)·154704a4(Windows cold mapper)로
수정했다. 실제 기존 API의 Exact1ns buffer/period RED2는 GREEN이며 platform415 PASS/
실패0·ignored2, sizing22·worker 외부 binary81·Source UI9·Windows bin72가 PASS다.
전체 portable app2774 PASS/실패0·ignored6이며 연결4는 그 안에 포함된다. UI의 새
24조합은 actual validator와 진짜 Owner/요청·응답 경계를 확인하며 cfgWindows mapper
실행을 대신하지 않는다. 새 MMCSS5개/18subcase와 전체 Windows platform/app lib/tests는
C-stub Rust 형 검사만 통과했고 실제 실행하지 않았다. scoped48 Rust format/diff와
non-denied library Clippy도 통과했다(기존 경고는 남음). 새 Source worker heap 검사도
실제 보정 뒤 통과했다.3개를 W→V로 복구해 Source12V·3N, 전체 D97·V78·W20·N3·
E14·P5·U3이다. 활성220·완료97/원래91/193은 유지하며 새 독립 DEEP 리뷰/QA가 남는다.
별도 상세 workspace5977 PASS/실패0·ignored38은 수정 전 a7869f70 결과이며 수정 후
전체 통과로 인용하지 않는다. 앞선 generic health0은 상세 진단을 버린 aggregate여서
이를 current 실행 실패의 원인 증명으로 사용하지 않는다.

E65: frozen9b6c5ae4의 두 번째/final 독립 DEEP 리뷰는 PASS/FIX_NOW0이며, 그 뒤
독립 CLI QA도 portable 소프트웨어 범위 PASS다. 실제 desktop,webtransport app3258/
실패0/ignored8, bin610/실패0/ignored17, platform415/실패0/ignored2, sizing22와
worker 외부 binary81, Source/Target publication 할당 각각1이 통과했다. worker
binary의 source-inclusion 중복과 app 내부 필터는 다른 suite와 합산하지 않는다.
browser-only WASM, Windows/macOS app/platform lib/tests C-stub typing, scoped48
Rust format/diff와 경고 비차단 library Clippy도 exit0이다. Clippy 경고는 앱394·
platform111개가 남으며 strict clean 판정이 아니다. 공개 CLI18경로도 통과했고 실제
BMS/WAV에서8000frame/32000byte PCM을 생성했다. 정확한 argv/exit와 로그는
`target/wf/retained-sourceframe-output-replacement/qa-cli-9b6c5ae4/`에 있다.

별도 실제 desktop QA는 기존 소유 Windows VM을 부팅하고 PowerShell Direct와
실제 IMMDeviceEnumerator의 Render AllStates 열거까지 성공했으나 endpoint가0개였다.
로그인 콘솔과 현재 Source 실행 파일/빌드 도구도 없어 실제 재생은 BLOCKED_ENV다.
VM은 초기 stopped/Off로 정상 복구했다. 증거는 같은 task의
`qa-desktop-9b6c5ae4/`다. Native COM/MMCSS fixture는 typed-only이며 미실행이다.
소프트웨어 범위8개(.01/.02/.03/.04/.06/.07/.08/.09)를 V→D로 옮긴다.
실제 Windows .05/.10/.11과 그 선행에 의존하는 공통 pump .12는 V를 유지한다.
Source8D·4V·3N, 전체 D105·V70·W20·N3·E14·P5·U3으로 활성220·47.73%다.
원래 제품 요구91/193·Source 부모·다른 backend/물리/출시 범위는 미완료 그대로다.
actual review/QA 결과와 별도로 hook-owned 완료 증거가 없어 한 번의 task_verify는
PENDING이다. 직접 수집한 결과는 NON-ATTESTING이며 task close/전체 완료로 쓰지 않는다.

E66: exclusive 후속의 실제 source 기반 독립 계획 검토 뒤 구현에 착수했다.
원래07.08.07.13을 portable worker/geometry, native complete lifecycle, Windows
consumer, 실제 장치 acceptance의4개 말단으로 분해했다. 원래 문장과 native 범위를
그대로 두고 실행 단위만 드러내 활성220→223이다. 시작 상태는 두 software lane W,
native 연동 N, 장치 검사 E이며 신규 완료는 없다. 전체 D105·V70·W22·N3·E15·P5·U3,
105/223(47.09%), 원래91/193(47.15%)은 그대로다. 이전 E65의220 분모는 당시 기록이다.
계약은 전체 native buffer와 whole-multiple 복구 PCM, Exact cold/actual geometry,
원래 Counter/cleanup 보존을 명시한다. 코드/실행/typing/독립 QA milestone은 각각
이4개 항목에서 갱신하며 계획 검토를 제품 검증으로 계산하지 않는다.

E67: portable source/test 쌍 동결 뒤 실제 worker36 PASS/실패0, sizing22와 외부
worker binary99 PASS를 확인했다. 기존18 worker와 신규18개가 모두 통과했으며
외부 binary는 중복 source-inclusion fixture와 실제 calibrated allocator1개를
포함하므로 suite 수를 합산하지 않는다. 새 exclusive geometry/full lease/복구
multiple·실패·conditional 경계의 측정 alloc/realloc/dealloc은0회다.
Windows planner79 PASS/실패0에는 explicit blank device가 기존 endpoint를 잘못
계승하는 실제 RED2→공통 parser 수정→GREEN2가 포함된다. 현재 native source/test
쌍은 실제 geometry와 prefill/event/cleanup 연결을 작성 중이다. .13.01을 W→V,
.13.02를 N→W로 갱신해 D105·V71·W22·N2·E15·P5·U3이며 완료/분모는105/223
그대로다. Windows cfg 실행/형 검사와 독립 review/QA 전 완료로 계산하지 않는다.

E68: native complete source/test 쌍도 동결하고 Windows platform/app의 lib/tests
C-stub Rust 형 검사를 exit0으로 확인했다. 새 Windows9개 fixture는 compile만
확인했으며 실행하지 않았다. 실제 GetBufferSize 뒤 history/reconfigure/MMCSS/
prefill 이전 guard와 Ready/event의 같은 selector가 production 호출점에 연결됐다.
기존 실제 common pump4개를 포함한 Source Owner/UI37 PASS와 Source/Target publication
할당 검사 각각1 PASS도 확인했다. 변경 Rust12개 format/diff와 문서 local link144개가
통과했다. native/consumer2개를 W→V로 옮겨 D105·V73·W20·N2·E15·P5·U3,
105/223(47.09%)다. 독립 최종 review/QA와 실제 Windows 장치 실행은 남는다.

E69: 사전 독립 coverage 점검의 shared cold-helper 우회 분기를 실제 production
helper를 호출하는 pending6/buffer4·shared3조합 owner 보존 회귀로 보강했다.
최종 공통 worker37·외부 worker binary100 PASS이며 중복 fixture는 합산하지 않는다.
platform/app의 경고 비차단 Clippy와 실제 windows_bms --help는 exit0이다.
독립 정식 review/QA와 실제 Windows 실행은 남는다. 신규 완료 없이 E68 집계를 유지한다.

E70: frozen bd9b8916의 실제 독립 DEEP 리뷰 PASS0 뒤 CLI 소프트웨어 QA가 PASS했다.
app3258/0/ignored8·bins624/0/ignored17·platform434/0/ignored2·worker37·외부100·
sizing22·Source/Target 할당 각1·WASM·Windows/macOS C-stub typing·경고 비차단
lib Clippy·scoped Rust12 format/diff·링크144와 공개 CLI26/26·동시 render를 확인했다.
실제 BMS/WAV는8000frames·32000bytes·비영샘플792·모든 값 finite로 렌더됐다.
외부100과 다른 suite의 중복 fixture는 합산하지 않는다. Windows 신규9개는 미실행이다.
새 desktop QA는 소유 VM의 실제 COM Render/AllStates 열거/GetCount가 성공했으나
endpoint0·로그인 콘솔/현재 player 부재로 BLOCKED_ENV였다. VM은 같은 incarnation의
초기Off로 정상 복구했다. software 범위 .13.01만 V→D로 옮겨 D106·V72·W20·N2·
E15·P5·U3,106/223(47.53%)다. 원래91/193·native V/E·exclusive/Source 부모와
다른 backend/물리/출시 요구는 유지하며 task/전체 Goal 완료를 주장하지 않는다.
근거는 현재 task의 qa-cli-bd9b8916/와 qa-desktop-bd9b8916/이며
요약과 재현 범위는 exclusive 변경 문서에 있다.

E71: selected-chart 키음 선택을 기존 local_preparation의 순수 함수 한 곳으로 옮기고
Windows/Linux/macOS local 호출부를 연결했다. 실제 준비 fixture7 PASS(기존5+새2),
retained-chart2와 cohort-setup7 PASS로 원본 PCM/BGM·전체 키음 복원·뒤로 이동·
멤버별 voice 분리와 end=None의 실제 completion을 확인했다. 필터 전 원본 맵은
선택 chart의 completion에서 실제 거절되므로 선택 view의 수명이 검증된다.
호스트 bin check와 Windows/macOS app/platform lib/tests C-stub typing은 exit0다.
WASM 기본 build도 exit0이며 browser feature와 최종 전체 앱 검사는 독립 QA에서
확인한다. foreign typing은 native 실행이 아니다. 새 네이티브 동작·효과 port·crate는
추가하지 않았다. 독립 coverage/contract·성능 자문에서 구체 결함은 찾지 못했으나
정식 review/QA를 대체하지 않는다.01.05/.06은 W, 전체106/223(47.53%)와 원래91/193은
그대로다. 남은 context/효과 주입/platform 자료형 의존과 전체 native 검증은 유지한다.

E72: frozen2b9928d3의 실제 독립 DEEP 리뷰 PASS0 뒤 독립 qa-cli가 순수 준비 정책과
소프트웨어 통합 범위 PASS했다. 전체 desktop,webtransport app3260/0/ignored8·
bins624/0/ignored17, focused 준비7·retained-chart2·cohort-setup7, host bins와 명시적
browser-feature WASM, Windows/macOS app/platform lib/tests C-stub typing이 통과했다.
focused 검사는 전체 suite와 중복이며 합산하지 않는다. scoped Rust5 format/diff·
WBS·문서 링크139개와 현재 소스로 빌드한 native CLI3개/12경로도 통과했다. CLI smoke는
도움말/인자 오류 검사이고 실제 helper 동작은 library/cohort 테스트로 확인했다.
경고 비차단 Clippy는 exit0/394 warnings이며 strict-clean이 아니다. foreign typing은
native ABI/장치/재생 증거가 아니다. 검사 전후 frozen10SHA와 tracked clean을 확인했다.
근거는 `target/wf/native-cohort-selected-sound-preparation/qa-cli-2b9928d3/`다.
01.05/.06의 전체 범위는 아직 W이며106/223·원래91/193은 유지한다. 자동 완료 기록과
owning verify/close는 별도 gate이고 이 소프트웨어 PASS가 task/전체 Goal 완료는 아니다.

E73(개발 당시): 순수 ConvertedBoundaryFacts의 정의를 기존 core audio로 옮기고 platform은
같은 타입을 직접 재노출한다. 공용 application/port10개 사용처는 core를 참조하며
필드·derive/default·생성 경계 의미와 native 상태/효과 실행문은 보존했다.
독립 작성한 양방향 public-path 대입 검사 포함 format23/0, core 경계7·mixer16,
platform434/0/ignored2와 app target_327/0/ignored1이 통과했다. host bins·명시적
browser-feature WASM·경고 비차단 app/platform Clippy와 Rust15 format/diff도 통과했다.
Windows/macOS C-stub Rust typing도 exit0이며 실제 장치/ABI/재생 증거는 아니다.
독립 정식 리뷰 다음 전체 앱 QA·최종 문서 audit는 아직 남는다.
기존 platform 출력 상태/presentation 의존성까지 제거한 것은 아니므로01.05/.06은 W,
전체106/223·원래91/193 그대로다. 정의 reflection 경로는 바뀌며 안정 ABI/layout·
이전 빌드와의 type identity나 실제 native 실행을 보증하지 않는다.
[변경과 검증 범위](../changes/2026-10-14-core-converted-boundary-facts.md)를 따른다.

E74: frozen7ac7a1af의 독립 DEEP 리뷰 PASS0 다음 독립 qa-cli가 DTO 이동의
소프트웨어 통합 범위 PASS했다. complete core746/0/ignored0, platform434/0/
ignored2·audio_format23/0, desktop,webtransport app3260/0/ignored8·bins624/0/
ignored17이다. target_에 빠지는 coordinate-epoch13개도 전체 suite에서 통과했다.
focused 검사는 중복이므로 합산하지 않는다. host bins·browser-feature WASM·
Windows/macOS C-stub Rust typing, Rust15 format/diff·WBS·문서 링크139개와
새로 빌드한 공개 CLI3개/12개 인자 경로를 통과했다. foreign/WASM은 build-only,
CLI는 도움말·인자 오류 증거이며 DTO 동작은 실제 library tests로 확인했다.
feature를 켠 Clippy는 exit0/app394·platform111 warnings이며 strict-clean이 아니다.
검사 전후20SHA·tracked clean을 확인했다. QA의 no-install 위반과 정확한 신규 설치
원복은 변경 문서에 기록한다.01.05/.06은 W,106/223·원래91/193을 유지하며 최종
문서 audit·owning verification/close와 실제 native/전체 player 완료는 별도다.

E75: CoreAudio retained Source 전체 callback/회수·backend/UI·solo/local 연결을
시작했다. 독립 계획 검토가 first-unsent telemetry, 승인 전 publication 금지,
전체 채널 검증 후 불가실패 copy, 실제 callback 최대 크기와 Mixer 성장 한계,
기존 telemetry와 충돌하지 않는 선택적 cold history 확장을 확인했다.
사전 REQ/ADR을 ef99f3a3에 커밋했고 서로 겹치지 않는 승인 CAS·history 확장에
각각 구현/테스트 작성자를 배치했다. .15는 N→W이며 아직 구현·검증 완료가 아니다.
이후 승인 기반8b9a63e9와 history 확장f8afbef9를 커밋했고 core history14/0,
platform library450/0/ignored2를 실행했다. portable production callback40b8821b의
별도19개도 통과했다. history 테스트2개의 잘못된 구간 분할 기대는 원래 covering
span 보존과 실제 pause 경계 이후 compaction을 확인하도록 고쳤다. native 수명주기
연결·실패 주입 fixture44개 작성 후 macOS C-stub lib/tests/macOS bin 형 검사를
통과했다. 처음 fixture 인자 오류와 외부 WASAPI allocation binary의 neutral module
누락은 수정했고 외부 binary116/0도 실행했다. C-stub에서 native fixture를 실행한
것은 아니다. backend/UI·solo/local·최종 리뷰/QA는 남는다.
분해 전 집계는106/223·원래91/193이었다. 현재 세부 기준선은 위 절을 따른다.
실제 runner 조회에서 owned Mac이 없고
macos-vm은 macos-host 누락으로 unavailable이다. native acceptance를 면제하거나
portable 실행·macOS C-stub typing을 장치 증거로 승격하지 않는다.
후속 b8f7494f에서 native lifecycle과 승인 후 관측 fence, variable maximum을
존중하는 기존 presentation 검사 확장을 커밋했다. platform 전체 library469/0/
ignored2와 macOS platform 형 검사 exit0을 확인했다. native audio48개와 추가
presentation6개, complete app backend의21개 fixture는 macOS 형 검사 대상이며
이 호스트에서 실제 native 실행한 결과가 아니다. app backend는 기존 port와
원래 packet qualifier를 공유하고 pending 실패의 matrix/capacity를 보존하며,
macOS app lib/tests/bin typing exit0이다. typed UI·solo/local 연결과 최종 독립
검토/QA·실제 Mac acceptance는 남는다.
2026-10-15 최종 software candidate2a7313c6: solo/local9e68bbb4·실제 최대
converter/등록 회귀32c5b78d·future 관측/승인 경합edc8294e까지 연결했다.
첫 정식 리뷰의 FAIL2를 해결한 뒤 두 번째 탐색 각0건과 정식 code/security PASS0,
그 다음 독립 CLI QA software PASS를 확인했다. core 전체749/0, platform473/0/
ignored2·format23/0·외부 WASAPI120/0, app3279/0/ignored8·bins640/0/ignored17·
외부 Source allocation1/0, host/WASM/Windows/macOS typing, source Clippy와
공개 CLI17/17이 통과했다. 외부 및 consumer 중복 실행 수는 새 coverage로 합산하지
않는다. native audio52·presentation 신규6·backend24·UI7은 cfg-Mac typed-only다.
실제 Mac/HAL/IOHID acceptance는 여전히 E이며 문서 audit·owning verify는 별도다.
[변경과 재현 근거](../changes/2026-10-14-coreaudio-retained-source-output.md)를 따른다.

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
- [ ] **BK-WBS-01.05** 모든 application context를 DDD 모듈과 명시적 Port/Adapter로 분리한다. — 상태=W(구현·연동중); 선행=-; 근거=E71/E72의 selected-sound 공통 정책과 E73/E74의 core 좌표 DTO·공용10개 소비 경로 및 독립 DEEP/CLI PASS; 필요 근거/다음=나머지 context·application/platform 자료형 의존성과 native 효과 경계 audit.
  - [x] **BK-WBS-01.05.01** 선택 채보의 local SoundBinding 정책을 기존 공통 준비 모듈 한 곳에서 소비하고 세 native 소비자가 원본 전체 맵을 복원한다. — 상태=D(검증완료); 선행=-; 근거=E71/E72, 2b9928d3 독립 DEEP0·CLI PASS와 실제 selected/full cohort·후방 복원 fixture; 범위=순수 정책 경계, native 장치 효과 아님.
  - [x] **BK-WBS-01.05.02** ConvertedBoundaryFacts의 정의는 core audio가 소유하고 공용 application/port 소비자는 같은 core 타입을 직접 사용한다. — 상태=D(검증완료); 선행=-; 근거=E73/E74, 7ac7a1af 독립 DEEP0·CLI PASS, public-path identity23·전체 core/platform/app 회귀; 범위=순수 DTO 정의/10개 소비 경로.
  - [ ] **BK-WBS-01.05.03** Source replacement application의 NativeOutputState/NativeFrameBasis 의존성과 presentation 자료형의 순수값·native 소유권 경계를 감사하고 필요한 분리를 끝낸다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=app/src/gameplay/output/application/coordinate_source_replacement.rs와 native_gameplay.rs/native_cohort.rs의 PresentationDiscipline 사용을 ADR__native-runtime-boundaries 기준으로 분류; 단순 DTO 이동으로 native owner를 core에 옮기지 않음.
  - [ ] **BK-WBS-01.05.04** 나머지 UI·설정·기록·경쟁·네트워크 context의 domain/application/port/adapter 소유권을 실제 import/효과 표와 대조하고 잔여 결합을 정리한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=app/src 각 context의 production import와 effect callsite를 조사해 doc/kernel/ADR__native-runtime-boundaries.md에 잔여 경계 및 bounded 후속 단위를 고정; cargo check -p beatkernel-bms-runtime --no-default-features --lib --locked.
- [ ] **BK-WBS-01.06** UI 제어·기록·파일·장치·시간·네트워크 효과를 주입 가능하게 만든다. — 상태=W(구현·연동중); 선행=-; 근거=E71/E72의 native-free 준비와 실제 cohort/복원 fixture·독립 QA; 필요 근거/다음=나머지 효과와 실제 adapter를 교체한 계층별 fixture.
  - [ ] **BK-WBS-01.06.01** 공통 gameplay/solo/cohort의 장치·입력·오디오 관측·HOST 효과를 주입 port로 교체한 fixture에서 같은 sequencing을 실행한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=native_gameplay.rs/native_cohort.rs/native_start.rs의 실제 공통 owner를 서로 다른 fake device로 실행; source/target·drain·취소·cleanup 오류가 같은 순서를 보존하는지 확인.
  - [ ] **BK-WBS-01.06.02** UI 제어·파일/기록 저장·네트워크 효과를 실제 context port에 주입하고 실패 전후 원본 상태와 저장 결과를 검사한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=native_finish.rs/native_cohort_setup.rs·player/UI·competition의 직접 효과 목록을 확정하고 context별 실제 adapter 대체 fixture를 연결; selected-sound 순수 fixture만으로 모든 효과 완료를 주장하지 않음.
  - [ ] **BK-WBS-01.06.03** 각 native adapter의 construction/cleanup과 공통 policy 사이 주입 경계를 실제 OS에서 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-01.06.01, BK-WBS-01.06.02; 필요 근거/다음=Windows/Linux/macOS 실제 입력/출력 장치와 native owner Stop/join/close 실패·취소를 공통 fixture의 결과와 대조; foreign typing은 실행 아님.
- [ ] **BK-WBS-01.07** hot path의 static dispatch와 추상화 비용을 실제 코드·측정으로 확인한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=동적 dispatch/clone/alloc 위치와 비용 보고.
  - [ ] **BK-WBS-01.07.01** Runtime 입력→binding→judge→오디오 명령 경로의 trait object·clone·allocation 위치를 실제 호출 그래프로 확정한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=rg로 dyn/Box/Arc/clone/Vec 생성 지점을 분류하고 core Runtime/입력 hot path의 setup-only 및 per-event 경계를 표로 기록; 기존 audio_rt_alloc만으로 전체 경로 완료를 주장하지 않음.
  - [ ] **BK-WBS-01.07.02** 공통 solo/cohort device와 output port의 static dispatch를 빌드 산출물과 호출 비용 측정으로 확인한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-01.07.01; 필요 근거/다음=동일 workload의 concrete/generic 호출을 release artifact에서 확인하고 입력 밀도·member 수별 시간/할당 비용을 기록; native_start/gameplay/cohort의 단형화와 제어 경로 boxing을 구별.
  - [ ] **BK-WBS-01.07.03** 준비된 callback의 성공·오류 경로 비용과 측정 한계를 actual native driver 조건까지 연결한다. — 상태=E(환경·장비대기); 선행=BK-WBS-01.07.01, BK-WBS-01.07.02; 필요 근거/다음=WASAPI/CoreAudio/ASIO/ALSA native callback 계측과 플랫폼별 deadline/late callback 통계; 전체 zero-cost 주장을 generic 선언만으로 하지 않음.
- [x] **BK-WBS-01.08** 순수 기록 모델을 graphics/browser 없이 빌드한다. — 상태=D(검증완료); 선행=-; 근거=E9.
- [ ] **BK-WBS-01.09** 미지원 기능은 capability/구체 오류로 표시하고 장치·모드·규칙을 몰래 대체하지 않는다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=native/browser capability 전체 경로 검사.
  - [ ] **BK-WBS-01.09.01** 설정·CLI·typed Output UI가 미지원 format/mode/backend/rule 선택을 retirement 이전에 구체 오류로 거절한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=SettingsHost별 schema, Windows target planner와 Source capability prevalidation의 요청/응답 및 live owner 보존을 current source로 재검사.
  - [ ] **BK-WBS-01.09.02** native/browser 시작과 복구가 선택 장치 identity·모드·규칙을 보존하고 대체 기능을 capability로 명시한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=default/explicit device, SDK-disabled ASIO, browser clock mapping·unsupported control 경로에서 실제 selected identity와 errors를 대조; fallback 없는 current E2E.
  - [ ] **BK-WBS-01.09.03** 실제 OS·browser에서 권한 거절·미지원 장치 조합이 사용자에게 같은 concrete capability/오류로 보인다. — 상태=E(환경·장비대기); 선행=BK-WBS-01.09.01, BK-WBS-01.09.02; 필요 근거/다음=Windows/MSVC/ASIO, macOS HAL/IOHID, Linux evdev/ALSA와 browser permission 거절 환경을 확보해 GUI/CLI 결과 확인.
- [ ] **BK-WBS-01.10** 독립 작업의 파일 소유권·선행 관계·통합·검증 장벽을 유지한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=병렬 작업 및 최종 통합 audit.
  - [ ] **BK-WBS-01.10.01** 병렬 변경의 파일 소유권·선행과 공유 compiler/모듈 등록 장벽을 실제 task 기록에 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=현재 task PLAN/agent ownership과 실제 git diff의 중복 파일을 대조; 하나의 source freeze와 공용 등록 담당을 확인.
  - [ ] **BK-WBS-01.10.02** 합쳐진 candidate의 source identity를 고정하고 독립 review→필수 QA→문서/verify 순서를 확인한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-01.10.01; 필요 근거/다음=각 독립 리뷰와 QA가 같은 frozen SHA/파일 hash를 검사했는지 audit; test 수 합산이나 작성자 self-check로 독립 gate 대체 금지.

### 02. 정수 시간·Transport·오디오 기준 시계

기능 연결: BK-001, BK-002, BK-021, BK-034. 요구: plan §§5, 17.1–2; REQ__native-target-time.

- [x] **BK-WBS-02.01** Timestamp/Duration 산술·음수·i64 경계 오류가 상태를 부분 변경하지 않는다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-02.02** 1x/0.5x/2x 및 1,000회 이상의 rate 변경이 기준 anchor 계산과 일치한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-02.03** pause/seek/reverse에서 원래 시간·이벤트 순서·snapshot 의미를 보존한다. — 상태=D(검증완료); 선행=-; 근거=E0.
- [x] **BK-WBS-02.04** 20시간/1주일 구간·큰 절대 좌표를 정수로 처리하고 표현 불가 범위는 거절한다. — 상태=D(검증완료); 선행=-; 근거=E0,E1,E2.
- [x] **BK-WBS-02.05** 다른 target rate 구간의 정확한 유리수 시간을 누적하고 마지막에 한 번만 floor한다. — 상태=D(검증완료); 선행=-; 근거=E2,E3.
- [x] **BK-WBS-02.06** 원본 clock domain/provenance를 보존하고 unknown mapping으로 판정을 진행하지 않는다. — 상태=D(검증완료); 선행=-; 근거=E0,E1.
- [ ] **BK-WBS-02.07** 실제 native/browser 플레이의 master clock을 출력 오디오 관측에 일관되게 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=source pull/HOST 시계를 출력 진행으로 대신하지 않는 E2E.
  - [ ] **BK-WBS-02.07.01** native Source Frame/Counter의 원래 출력 관측만으로 공통 solo/cohort 판정 frontier를 진행한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=native_audio_presentation·native_gameplay/native_cohort의 실제 Source consumer를 확인; pulled/rendered frontier나 HOST 시각이 deadline 권한을 주지 않는 regression 재실행.
  - [ ] **BK-WBS-02.07.02** converted Target의 native presentation과 source song grid를 공통 authority에서 구별한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E20–E23 actual ConvertedAlsaStream 시작·BGM·input authority 소비 경로를 current source로 확인; source pull/target generation/admission/presentation을 별도 oracle로 비교.
  - [ ] **BK-WBS-02.07.03** browser Worker/Worklet의 실제 출력 관측과 Window interpolation을 분리해 master clock을 유지한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=actual current WASM→Worklet→Worker/native mapping runner에서 pause/resume/idle/seek와 stale packet을 검사; browser HOST fallback 거절.
  - [ ] **BK-WBS-02.07.04** 실제 native endpoint의 출력 진행 정지·drift가 logical master clock을 거짓 진행시키지 않는다. — 상태=E(환경·장비대기); 선행=BK-WBS-02.07.01, BK-WBS-02.07.02, BK-WBS-02.07.03; 필요 근거/다음=clock-capable Windows/Linux/macOS endpoint에서 full play/pause/end를 실행; browser 실제 Worklet 검사는 .03에서 별도 진행하며 null과 host timer는 native presentation 증거가 아님.
- [ ] **BK-WBS-02.08** backend/rate/buffer/latency 변경 시 epoch·입력 mapping·song origin을 원자적으로 갱신한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=실제 교체 경로·실패/재시도·다른 backend 검사.
  - [ ] **BK-WBS-02.08.01** Target 교체가 authority/native/pause/input merger의 원래 epoch·origin을 borrowed atomic commit으로 바꾼다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E51/E55의 controller18·allocation0 검사를 current 합쳐진 source로 재확인; BK-WBS-07.08.05와 같은 증거를 참조하되 전체 backend 완료와 구분.
  - [ ] **BK-WBS-02.08.02** Source Frame/Counter 교체가 새 native descriptor·정확한 frequency·history pin을 원래 song origin과 함께 갱신한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E65 Source controller/epoch·1ns tamper/stale refusal와 새 packet proof 회귀; BK-WBS-07.08.07.01/.02/.07의 D 범위를 참조.
  - [ ] **BK-WBS-02.08.03** rate/buffer/latency 변경 요청의 실패·취소·재시도가 기존 epoch/mapping/origin을 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=settings/capability/applied 변경별 common Owner 회귀와 browser rate/buffer 경로를 대조; wrong-domain/stale과 pending PCM 해석 변경 거절을 검사.
  - [ ] **BK-WBS-02.08.04** 실제 backend 전환과 장치 변경의 새 native 관측으로 epoch 교체·resume을 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-02.08.01, BK-WBS-02.08.02, BK-WBS-02.08.03; 필요 근거/다음=지원 OS 실제 endpoint에서 두 progressing 관측→atomic publication→input replay/capture 비교; WASAPI↔ASIO SDK/driver와 CoreAudio HAL 필요.
- [ ] **BK-WBS-02.09** 동일한 정수 판정 시간과 시각 보간 시간을 분리해 장시간 시각 drift를 검사한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=[장시간 시각 계약](REQ__long-duration-visual-time.md): 큰 과거 SV 및 마커 교차 정밀도 결함을 indexed 상대 구간 합으로 수정. 실제 20시간/1주일 가상 traversal·Runtime/replay 정수 identity·app retained geometry는 독립 코드 리뷰·CLI QA PASS(코어 전체 451, 신규 코어 8, app 3 및 기존 playfield 3); 실제 GPU/physical drift 증거는 남음.
  - [x] **BK-WBS-02.09.01** 20시간/1주일 시점에서 큰 과거 SV가 현재 head/tail의 국소 scroll 적분을 잃지 않도록 indexed 상대 구간 합을 검증한다. — 상태=D(검증완료); 선행=-; 근거=REQ__long-duration-visual-time, 6f7567f 독립 DEEP·CLI PASS; 신규 core8, marker crossing RED→GREEN, 전체 core435+doc16; 범위=CPU projector geometry.
  - [x] **BK-WBS-02.09.02** 원점부터 전체20시간/1주일 가상 traversal에서 Runtime 판정·pause·genuine replay의 정수 identity를 보존한다. — 상태=D(검증완료); 선행=-; 근거=REQ__long-duration-player-traversal, a61b03a 독립 code/security·CLI6/6·36경로, 실제168h604800000 playback samples와 original capture/replay; 범위=1000Hz sparse software traversal.
  - [x] **BK-WBS-02.09.03** app PlayfieldCache가 큰 절대 시각·pause·epoch rebase·후방 seek에서 bounded local head/tail geometry와 drift 오차를 유지한다. — 상태=D(검증완료); 선행=-; 근거=REQ__long-duration-visual-time 독립 CLI app3 및 기존 playfield_gpu3 PASS, renderer-input tolerance0.001px; app이 core projector를 소비한다는 주장은 아님.
  - [ ] **BK-WBS-02.09.04** 실제 GPU 렌더링에서 장시간 시각 보간과 동일 정수 판정 시각의 drift를 측정한다. — 상태=E(환경·장비대기); 선행=BK-WBS-02.09.01, BK-WBS-02.09.03; 필요 근거/다음=실제 native/browser renderer에서 long-duration geometry·frame cadence를 출력 이미지/정수 song trace와 비교; CPU fixture만으로 GPU pixel 완료 처리 금지.
  - [ ] **BK-WBS-02.09.05** 실제 출력/입력 장치의 장시간 wallclock 운전에서 visual/audio drift와 정수 판정 provenance를 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-02.09.02, BK-WBS-02.09.04; 필요 근거/다음=20시간/1주일 실제 wallclock soak와 audio loopback/입력 trace; virtual traversal은 물리 시간/음향 증거가 아님.
- [ ] **BK-WBS-02.10** 향후 pre-play 입력 보존/폐기 규칙을 확정하고 metadata/recording 의미를 문서화한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=기존 미응답 정책 질문; 현재 동작 보존.
  - [ ] **BK-WBS-02.10.01** pre-play에 획득한 원본 Down/Up·held 상태를 보존할지 폐기할지와 경계 시각의 정책을 확정한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=사용자가 미응답한 pre-play 정책 선택을 확정; 현재 동작을 보존하고 임의 default 추가 금지.
  - [ ] **BK-WBS-02.10.02** 확정한 pre-play 정책을 recording metadata/replay identity와 실제 시작 시 held/release 의미에 고정한다. — 상태=P(정책선택대기); 선행=BK-WBS-02.10.01; 필요 근거/다음=정책 결정 후 metadata version/기록 호환과 입력 시작 경계 fixture를 문서화; empty-POOR dialect와 구별.

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
  - [ ] **BK-WBS-03.10.01** 고정/동적 source 등록과 선택 device identity가 분리·재등장 때 원본 sequence chronology를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=REQ__bms-local-input의 cumulative1..4096 dynamic slots와 exact selected attachments를 native/browser consumer에서 대조; removal 뒤 identity 재사용/sequence 초기화 금지.
  - [ ] **BK-WBS-03.10.02** OS별 hotplug·permission/disconnect 오류가 collector drain/cut/cleanup과 함께 상위 owner에 전달된다. — 상태=E(환경·장비대기); 선행=BK-WBS-03.10.01; 필요 근거/다음=Raw Input/evdev/hidraw/IOHID 실제 장치를 detach·권한 거절하고 acquired prefix와 join/close 결과를 확인.
- [ ] **BK-WBS-03.11** 1인 자동 입력과 2..64인 명시 할당을 native/browser 실제 시작에 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=UI roster와 native source identity E2E.
  - [ ] **BK-WBS-03.11.01** 1인 자동 입력이 실제 native acquisition의 device/source identity를 동적으로 등록해 공통 merger에 전달한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=native_defaults와 native collector의 실제 ID→dynamic source admission 경로 확인; aggregate/fabricated identity 및 강제 단일 device selector 금지.
  - [ ] **BK-WBS-03.11.02** 2..64인 명시 roster가 native/browser 실제 start에서 exact-device binding과 member별 입력/voice/capture를 유지한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=UI roster→native_cohort_setup→RuntimeGroup/native/browser start를 실행하고 중복/미할당/잘못된 source의 cold refusal 확인.
  - [ ] **BK-WBS-03.11.03** 실제 복수 native 입력 장치에서 UI 할당과 source/member identity를 end-to-end 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-03.11.01, BK-WBS-03.11.02; 필요 근거/다음=두 실제 장치 동일 HID key의 fanout/독립 member capture를 actual native 시작에서 확인;64 pure fixture는64physical 장치 증거 아님.
  - [ ] **BK-WBS-03.11.04** 실제 browser UI roster에서1인 자동 입력·2인 이상 명시 할당으로 시작하고 원본 source/member를 확인한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-03.11.02; 필요 근거/다음=current WASM의 실제 browser runner로 roster 편집·중복/미할당 거절·same-control distinct source 입력과 member capture를 확인; 물리 native 장치 대기를 browser 소프트웨어 blocker로 사용하지 않음.
- [ ] **BK-WBS-03.12** 키보드·HID·고주파 입력의 실제 OS timestamp 의미와 손실/순서를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=Windows/Linux/macOS 장치·1kHz 이상 source.
  - [ ] **BK-WBS-03.12.01** Windows Raw Input 키보드/HID 원본 timestamp·sequence와 손실/순서를 고주파 source에서 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=Windows 실제 keyboard/HID와1kHz 이상 source를 확보해 acquisition timestamp domain·FIFO·overflow/disconnect trace 기록.
  - [ ] **BK-WBS-03.12.02** Linux evdev/hidraw의 kernel timestamp 의미·drain·loss를 고주파 실제 source에서 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=/dev/input·/dev/hidraw 실장치로 EV_SYN/SYN_DROPPED·report sequence 및 acquired prefix를 기록; UI frame timestamp 대체 금지.
  - [ ] **BK-WBS-03.12.03** macOS IOHID timestamp·Mach domain·checked queue 손실을 실제 source에서 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=owned Mac/IOHID 장치와1kHz source에서 queue/runloop/drain 및 원본 metadata trace; macos-host 부재는 blocker로 유지.

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
  - [ ] **BK-WBS-04.10.01** core input/codec/binding/compiler/judge 오류와 capacity 거절이 원본 상태를 부분 변경하지 않는지 감사한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=crates/beatkernel/tests의 malformed/overflow/capacity fixture를 error→state oracle 표로 묶고 미검사 mutation 순서를 확인; current core suite 실행.
  - [ ] **BK-WBS-04.10.02** Runtime/group 명령 발행·credit·ACK 오류의 accepted prefix와 side-effect 순서를 감사한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 Runtime→Mixer queue/finite seal에서 partial prefix·queue full·later failure를 검사; 이미 승인된 효과와 미승인 상태를 구분.
  - [ ] **BK-WBS-04.10.03** custom/trusted callback 경계의 panic·오류·side-effect 허용 계약을 실제 caller와 대조한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=custom judge/device/payload callback의 호출 전 검증·panic propagation·retry 의미를 source audit; 순수성을 trait만으로 보장하지 않음.
- [ ] **BK-WBS-04.11** 현재 source 전체에서 live class/EX/accepted lane 피드백을 실제 GUI와 일치시킨다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 Runtime prefix→UI→결과 비교.
  - [ ] **BK-WBS-04.11.01** 실제 Runtime accepted prefix의 opaque class·EX·accepted lane 피드백이 live model에 정확히 반영된다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/src/judge_feedback.rs와 native score/gameplay host에서 miss/repeat/refused/partial prefix의 class/EX/lane oracle 비교.
  - [ ] **BK-WBS-04.11.02** solo/local 및 replay 결과 화면의 class·EX·lane 표시가 원본 capture와 동일 member별 결과를 사용한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=RuntimeGroup distinct member·genuine capture/replay를 UI model과 결과 archive에 연결; grade 추정이나 UI 재판정 금지.
  - [ ] **BK-WBS-04.11.03** 실제 native GUI에서 live class·EX·lane 표시와 최종 capture/결과를 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-04.11.01, BK-WBS-04.11.02; 필요 근거/다음=current native binary와 실제 입력/output 장치의 플레이를 캡처해 live 표시→원본 기록→결과를 대조; 순수 model suite는 native 화면 증거 아님.
  - [ ] **BK-WBS-04.11.04** 실제 browser GUI에서 live class·EX·lane 표시와 replay/최종 결과를 대조한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-04.11.01, BK-WBS-04.11.02; 필요 근거/다음=current WASM→Worker→renderer 실제 browser runner로 accepted prefix·member별 class/EX/lane·capture/replay/결과를 확인; native 장치 부재와 별개로 실행.
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
  - [ ] **BK-WBS-05.11.01** WASAPI shared/exclusive production worker의 fill·clock-only·retirement·오류 경로에서 heap/lock/IO 경계를 감사한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E65/E70 portable production37·외부 allocation binary100·heap0을 기준으로 현재 COM wrapper까지 callsite audit; cargo test -p beatkernel-platform --test wasapi_source_worker_allocations --locked.
  - [ ] **BK-WBS-05.11.02** CoreAudio complete/legacy callback의 모든 channel·retirement·copy 실패 경로에서 준비와 callback 비용을 구분한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=40b8821b portable callback19·calibrated heap0을 기준으로 actual macos io_proc wrapper의 allocation/deallocation/lock/IO callsite audit; BK-WBS-07.08.07.15.03 참조.
  - [ ] **BK-WBS-05.11.03** ASIO callback과 ALSA 생산 경로의 성공·쓰기 실패·clock 오류에서 hot allocation/lock/IO 책임을 감사한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=crates/beatkernel-platform/src/audio/asio/render.rs 및 Linux output pump의 실제 callback/owner 경계와 source publication을 측정; ASIO SDK branch는 별도.
  - [ ] **BK-WBS-05.11.04** 실제 driver callback의 비용·deadline·오류 경로 계측을 플랫폼별 장치에서 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-05.11.01, BK-WBS-05.11.02, BK-WBS-05.11.03; 필요 근거/다음=실제 WASAPI/CoreAudio/ASIO/ALSA driver와 준비된 계측으로 callback heap/lock/IO 및 cleanup 위치를 기록; foreign type check는 runtime audit 아님.
- [ ] **BK-WBS-05.12** 미지원 format/channel/rate/period와 native 쓰기 실패를 구체 오류로 반환한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=모든 backend 실제 applied-config 검사.
  - [ ] **BK-WBS-05.12.01** format/channel/rate/period 요청을 cold preflight와 capability에서 구체 오류로 거절한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=audio negotiation/request sizing·matrix·Exact duration·unsupported encoding의 원본 요청/적용 가능성을 backend별 검사; silent coercion 금지.
  - [ ] **BK-WBS-05.12.02** actual applied format/geometry가 요청과 다른 경우 backend가 typed mismatch를 보존하고 owner를 안전하게 반환한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=WASAPI GetBufferSize/period·CoreAudio actual maximum·ALSA applied hw params·ASIO actual geometry를 구체 error/complete recovery와 대조.
  - [ ] **BK-WBS-05.12.03** native write/release/start/stop 실패의 operation/code와 uncertain admission을 실제 장치에서 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-05.12.01, BK-WBS-05.12.02; 필요 근거/다음=backend별 실제 실패/분리·unsupported 요청을 유발해 원본 오류와 cleanup 오류, PCM quarantine 및 user-visible diagnostic을 확인.
- [ ] **BK-WBS-05.13** pitch 변화 허용 배속의 sample read-head/resampling과 명령 rate가 선언한 재생 속도에 맞는 PCM을 만든다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-05.01; 필요 근거/다음=plan §11.4; [offline replay 배속 독립 검증](../changes/2026-10-12-replay-render-playback-speed.md)은 shared planner·실제 PCM·DEEP code/security PASS·QA40테스트/optimized CLI54경로까지 확인; [고정 배속 common/native/browser replay 독립 검증](../changes/2026-10-12-realtime-replay-speed.md)은 DEEP code/security·CLI·실제 browser/UX PASS 및 WASM·Worker·Worklet·renderer 네 배속과 native pause/null/외국 OS 타입 검사를 확인; null startup은 자연종료/음향 증거가 아님; live/runtime 배속 전환·retained practice·물리 출력의 속도·시계·pause/seek 연동도 남음; device sample-rate 변경을 playback speed로 대신하지 않음.
  - [x] **BK-WBS-05.13.01** offline replay의 고정 양수 배속이 shared planner와 실제 sample read-head/resampling PCM에 일치한다. — 상태=D(검증완료); 선행=-; 근거=2026-10-12-replay-render-playback-speed.md 독립 DEEP code/security PASS·CLI QA40테스트/optimized54경로; 범위=offline positive pitch-changing replay.
  - [x] **BK-WBS-05.13.02** 공통/native/browser recorded replay의 고정 배속을 출력시간·pause·원본 recording identity와 실제 PCM까지 검증한다. — 상태=D(검증완료); 선행=-; 근거=2026-10-12-realtime-replay-speed.md fc2376a 독립 code/security·CLI/browser/UX PASS, 실제 네 배속 Worker/WASM/Worklet/renderer; native null은 자연종료/음향 증거 아님.
  - [ ] **BK-WBS-05.13.03** 실제 live solo/local의 고정 배속이 Transport 분수 위상·BGM/key 명령·finite-end credit와 capture를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=REQ__live-playback-speed 및2ce281d DEEP PASS 이후 필수 QA 완료; native3/2·NORMAL, pause/resume, expired/applied/consumed와 original provenance를 current source에서 확인.
  - [ ] **BK-WBS-05.13.04** retained practice의 F5·loop/scrub·Source/Target controls가 선택 배속·원본 section/attempt/capture를 유지한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=2026-10-12-retained-playback-speed.md CLI4763/0·24경로 및 DEEP/security PASS를 기준으로 qualified native retained play/controls/drain과 남은 task QA를 수행.
  - [ ] **BK-WBS-05.13.05** retained browser의 delayed touch·in-flight prefix·audio WASM fetch 오류를 해결하고 실제 practice 흐름을 재검증한다. — 상태=W(구현·연동중); 선행=BK-WBS-05.13.04; 필요 근거/다음=동일 문서 strict browser10/14 FAIL과 delayed-touch diagnostics의 원본 reproduction을 유지하고 actual main.js/Worker/Worklet 경로 수정→전체 strict runner 통과; ERR_ABORTED를 제외하지 않음. 현재=doc/changes/2026-10-15-browser-audio-current-acceptance.md의 fresh13journey2회 strict FAIL·공식 Chromium153 pre-fix loader 확인; test oracle/CDP cleanup 수정은 완료했으나 delayed touch/in-flight 복구와 fixed-engine network acceptance는 남음.
  - [ ] **BK-WBS-05.13.06** 실행 중 배속 전환의 sample read-head·명령 rate·Transport/seek/pause 정책을 구현한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=REQ__runtime-playback-rate.md·ADR__runtime-rate-phase-and-receipts.md 및 2026-10-15-runtime-playback-rate.md 참조. 공통 phase/PCM/Transport/가변 기록과 native Source/Target solo/cohort·6곳 cold 조립·browser Step/Worker/Worklet/UI를 연결했다. signed pre-arm 관측·전체 voice/BGM 예산·closure-only phase·ACK 뒤 재관측 경합을 수정했고 scoped Step34/0·native51/0·Node324/0·실제 Rust Worklet24/0을 확인했다(겹치는 검사는 더하지 않음). 열세 번째 실제 browser80375는 리뷰 전268bb73d JS·matching fresh WASM에서 필수12개 흐름 모두 PASS, 추가 저장 검사 포함13checks·unmet0·console/network0·owned browser/Xvfb exit0이다. 268bb73d의 독립 리뷰5건은 c32d1287·caaaba03·f9243be5에서 수정했다. 현재 Node368/0·기록 조회 Rust67/0·별도 focus24/0과 실제 두 WASM/matching bindgen을 확인했다(겹치는 검사는 합산하지 않음). 새 session 키·게임 메뉴 Records·editor release를 포함한 열네 번째 실제 browser83650은 필수14개/전체15checks PASS, console/network0·owned browser/Xvfb exit0이며 runner fc14ca44로 커밋했다. 후속 독립 code/security는 동일한 기록 회수 결함2건(늦은 수락 ACK 보존, 실패 시 retired capture 보존)을 수정 필요로 판정했다. AC032/033은 a4bad974·bfb6affb로 수정했고 현재 Node376/0·core codec7/0·앱 관련5suite58/0(실제 solo/local 인코딩 할당 실패와 원본 회수 포함)이 통과했다. 다음=새 WASM/browser 확인, 새 정식 code/security review와 browser/CLI/desktop QA 및 최종 owning gate. 개발 검사로 독립 QA나 실제 장치/SDK·음향·지연·성능 acceptance를 대신하지 않으며 물리 범위는 .07 E로 남긴다. 전체136/489·오디오 커널12/27 유지.
  - [ ] **BK-WBS-05.13.07** 실제 출력 장치에서 고정/retained/전환 배속의 PCM 속도·시계·pause/seek alignment를 측정한다. — 상태=E(환경·장비대기); 선행=BK-WBS-05.13.03, BK-WBS-05.13.04, BK-WBS-05.13.05, BK-WBS-05.13.06; 필요 근거/다음=clock-capable native 장치·실제 browser audio와 loopback에서 배속별 PCM/pitch/시간을 기록; device sample-rate 변경으로 playback speed를 대신하지 않음.
- [ ] **BK-WBS-05.14** ReverseTimelineOnly/ReverseSamples/Mute 선택 정책의 명령·원본 sample 방향·무음 결과를 검증한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-09.02; 필요 근거/다음=core runtime/playback.rs 및 reverse_playback fixture를 현재 scope로 확인; Transport reverse만으로 sample 정책을 완료 처리하지 않음.
  - [ ] **BK-WBS-05.14.01** ReverseTimelineOnly가 복원된 역방향 timeline에서 원본 sample 방향과 명령 정책을 유지하는지 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=crates/beatkernel/tests/reverse_playback.rs literal_pcm_policies_restore_identical_logical_state 및 normal_to_reverse_transition_changes_pcm_without_changing_judge를 current source로 실행.
  - [ ] **BK-WBS-05.14.02** ReverseSamples가 원본 PCM read-head를 역방향으로 읽으면서 동일 logical judge/replay 상태를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=cargo test -p beatkernel --test reverse_playback --locked; 비대칭 literal PCM oracle와 seek/checked rate·queue refusal 복원 상태 확인.
  - [ ] **BK-WBS-05.14.03** Mute가 과거 future 명령/voice를 남기지 않고 같은 역방향 논리 결과에서 실제 무음을 만든다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=reverse_playback의 mute_replacement_has_no_old_future_pending_commands 및 queue_failure_does_not_change_reconstruction_or_retry 실행; Transport reverse만으로 sample 정책 완료 처리 금지.

### 06. 플랫폼별 Native IO backend

기능 연결: BK-012, BK-013, BK-014, BK-015, BK-016, BK-017, BK-057. 요구: plan phases4/7/12/13; platform REQ.

- [ ] **BK-WBS-06.01** Windows Raw Input 키보드/HID 획득과 native owner 종료를 최신 source로 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=현재 Windows 실행; 과거 VM 결과만 확대 인용 금지.
  - [ ] **BK-WBS-06.01.01** 현재 Windows Raw Input 표준 keyboard/HID 획득이 원본 device/control/timestamp/sequence를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=doc/kernel/REQ__windows-input.md와 Windows acquisition production source/fixtures를 current source로 대조; 과거 VM 결과의 scope 확대 금지.
  - [ ] **BK-WBS-06.01.02** 실제 Windows 입력 owner가 권한/분리·cancel/drain/종료에서 모든 native 자원을 회수한다. — 상태=E(환경·장비대기); 선행=BK-WBS-06.01.01; 필요 근거/다음=현재 Windows binary와 실제 Raw Input 장치를 실행해 acquired prefix·native window/thread join 및 cleanup 오류를 확인.
- [ ] **BK-WBS-06.02** Windows WASAPI shared stream·format/period/device/clock 제어를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 드라이버/applied-config/회수.
  - [ ] **BK-WBS-06.02.01** WASAPI shared의 device/format/engine period 선택과 actual applied buffer/Counter descriptor를 확인한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=crates/beatkernel-platform/src/windows/audio.rs와 windows_audio suite, shared complete lifecycle의 native-gated tests를 current Rust typing과 별도 실행으로 검증.
  - [ ] **BK-WBS-06.02.02** 실제 shared endpoint의 start/presentation/Stop/join 및 recoverable failure를 검증한다. — 상태=E(환경·장비대기); 선행=BK-WBS-06.02.01; 필요 근거/다음=eligible Windows native output endpoint가 필요; 현재 endpoint0 blocker를 유지하고 실제 IAudioClock·MMCSS·cleanup 결과 확인.
- [ ] **BK-WBS-06.03** Windows WASAPI exclusive stream·지원 거절·buffer 크기를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 exclusive device matrix.
  - [ ] **BK-WBS-06.03.01** WASAPI exclusive event mode의 exact geometry·whole buffer fill·지원 거절을 현재 native 코드와 대조한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E70 portable worker37/geometry 검증과 actual GetBufferSize/period equality·exclusive timer unsupported 경계를 audit; shared buffer 규칙을 확대하지 않음.
  - [ ] **BK-WBS-06.03.02** 실제 exclusive endpoint에서 적용 buffer·presentation·pause/end 및 완전 cleanup을 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-06.03.01; 필요 근거/다음=eligible Windows exclusive 장치로 actual COM 경로 실행; native9개 미실행/endpoint0을 portable PASS로 대체하지 않음.
- [ ] **BK-WBS-06.04** 선택적 ASIO SDK/MSVC bridge·채널/rate/buffer granularity·callback/clock을 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 SDK 빌드와 설치된 신뢰 driver.
  - [ ] **BK-WBS-06.04.01** 선택적 ASIO SDK/MSVC bridge가 배포 조건을 지키며 진짜 SDK branch를 빌드·링크한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=REQ__asio-distribution·ASIO SDK/MSVC 환경과 신뢰 driver를 준비해 실제 C++ bridge build/link; GNU C-stub typing 제외.
  - [ ] **BK-WBS-06.04.02** ASIO 채널/rate/buffer granularity와 실제 callback/clock interval이 정확한 capability로 전달된다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=asio_configuration/clock_sources/render suites와 실제 staged geometry·multimedia clock/error bound source를 대조; 실제 SDK branch 실행은 선행 환경 필요.
  - [ ] **BK-WBS-06.04.03** 실제 ASIO driver에서 callback start/end·clock·stop/join과 실패 회수를 실행한다. — 상태=E(환경·장비대기); 선행=BK-WBS-06.04.01, BK-WBS-06.04.02; 필요 근거/다음=설치된 신뢰 ASIO driver에서 실제 callback/interval 및 owner cleanup을 기록; 설치/다운로드를 task가 자동 수행하지 않음.
- [ ] **BK-WBS-06.05** Linux evdev 표준 키·장치 identity·분리/권한·drain을 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 evdev 장치.
  - [ ] **BK-WBS-06.05.01** evdev 표준 HID key와 source identity·FIFO acquired prefix·drain 계약을 현재 adapter와 대조한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=Linux native input collector와 REQ__native-input-collector/REQ__bms-local-input의 evdev clock/source 의미 확인.
  - [ ] **BK-WBS-06.05.02** 실제 evdev 분리·권한 거절·SYN_DROPPED가 drain/종료 오류와 함께 전달된다. — 상태=E(환경·장비대기); 선행=BK-WBS-06.05.01; 필요 근거/다음=/dev/input 실제 장치와 권한이 필요; identity 보존·backlog barrier·join을 actual acquisition trace로 확인.
- [ ] **BK-WBS-06.06** Linux hidraw report 획득·adapter·손실/종료를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 HID 장치.
  - [ ] **BK-WBS-06.06.01** hidraw 원본 report가 실제 DeviceAdapter로 전달되고 malformed report·순서·손실 오류를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=Linux hidraw acquisition→canonical adapter→collector queue의 current source/fixture를 대조; 단순 raw decoder fixture와 OS read를 구별.
  - [ ] **BK-WBS-06.06.02** 실제 hidraw 장치의 고주파 report·분리·종료와 collector 자원 회수를 검증한다. — 상태=E(환경·장비대기); 선행=BK-WBS-06.06.01; 필요 근거/다음=/dev/hidraw 장치에서 report payload/sequence·overflow/disconnect·close/join trace 기록.
- [ ] **BK-WBS-06.07** ALSA device/format/period/buffer·timestamp·xrun·완전 회수를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 출력 장치; null은 음향 증거 아님.
  - [ ] **BK-WBS-06.07.01** ALSA requested/applied device/format/period/buffer 및 timestamp capability가 원본 요청과 일치한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=ALSA production negotiation과 null software fixture를 current source로 실행; encoding/pending interpretation 거절·recovery와 실제 applied params를 분리.
  - [ ] **BK-WBS-06.07.02** 실제 ALSA 출력에서 timestamp·xrun/suspend recovery와 complete state 회수를 검증한다. — 상태=E(환경·장비대기); 선행=BK-WBS-06.07.01; 필요 근거/다음=/dev/snd clock-capable endpoint와 실제 xrun/분리 조건에서 PCM admission·timestamp·Stop/recovery 확인; null은 음향 증거 아님.
- [ ] **BK-WBS-06.08** macOS IOHIDManager/checked queue와 모든 선택 source의 drain을 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 키보드/HID와 runloop/queue 오류.
  - [ ] **BK-WBS-06.08.01** IOHIDManager/checked queue가 모든 선택 source의 원본 Mach timestamp·device identity·drain cut을 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=macOS IOHID production code와 collector fixture를 current source로 대조; selected attachment 확인/queue/runloop error 전파 검사.
  - [ ] **BK-WBS-06.08.02** 실제 Mac 입력 장치의 runloop/queue 실패·분리·권한 거절·종료를 실행한다. — 상태=E(환경·장비대기); 선행=BK-WBS-06.08.01; 필요 근거/다음=owned Mac/IOHID device 필요; 현재 macos-host 부재를 유지하고 all-selected-source drain과 stop/join을 trace로 확인.
- [ ] **BK-WBS-06.09** CoreAudio device/format/buffer·callback·presentation interval·회수를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 macOS 출력 장치.
  - [ ] **BK-WBS-06.09.01** CoreAudio exact device/format/nominal/variable maximum과 presentation interval의 source 계약을 대조한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=REQ__coreaudio-presentation/REQ__coreaudio-render-telemetry와 actual macos/audio.rs의 Mach/sample/flags·actual buffer geometry를 audit; complete 변경은07.08.07.15 참조.
  - [ ] **BK-WBS-06.09.02** 실제 HAL callback start/stop/unregister/drain과 원래 presentation interval·회수를 검증한다. — 상태=E(환경·장비대기); 선행=BK-WBS-06.09.01; 필요 근거/다음=owned Mac/HAL output device가 필요; portable callback19 및 C-stub Rust typing을 native lifecycle 실행으로 세지 않음.
- [ ] **BK-WBS-06.10** 각 OS default device와 명시 override를 자동/정확 선택 계약으로 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=native 장치 선택과 unavailable 오류.
  - [ ] **BK-WBS-06.10.01** OS default 장치 탐색과 명시 override가 하나의 native_defaults 준비에서 정확한 identity를 결정한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=native_defaults/auto_native 및 각 launcher에서 missing/explicit/default 요청을 대조; invocation lineage와 retry pin이 재탐색 없이 선택을 보존하는지 확인.
  - [ ] **BK-WBS-06.10.02** 장치 unavailable/default 변경이 실제 시작과 Output UI에서 concrete 오류 또는 실제 새 default로 처리된다. — 상태=E(환경·장비대기); 선행=BK-WBS-06.10.01; 필요 근거/다음=각 OS 실제 장치와 default 변경 환경에서 omitted device·명시 blank·explicit identity 동작을 실행; Windows WASAPI blank default와 ASIO CLSID를 구별.
- [ ] **BK-WBS-06.11** format/rate/channel/latency/period capability와 실제 적용 결과를 공통 UI에 표시한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=요청과 applied 차이를 숨기지 않는 검사.
  - [ ] **BK-WBS-06.11.01** 공통 Settings/Output capability가 format/rate/channel/latency/period 요청과 backend별 지원 범위를 정확히 표현한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=각 SettingsHost schema·OutputCapability·native UI adapter의 fields/current_args roundtrip을 current source로 검사; unsupported fields가 inert/explicit refusal인지 확인.
  - [ ] **BK-WBS-06.11.02** Apply 후 실제 backend applied format/geometry와 새 capability가 동일 요청 ID의 응답으로 갱신된다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=actual Owner/controller→typed reply→capability refresh fixture에서 requested/applied 차이, WouldBlock와 reopened draft를 확인.
  - [ ] **BK-WBS-06.11.03** 실제 native GUI에서 backend가 바꾼 applied 값과 미지원 옵션 오류를 표시한다. — 상태=E(환경·장비대기); 선행=BK-WBS-06.11.01, BK-WBS-06.11.02; 필요 근거/다음=admitted paused 출력과 실제 장치가 필요; 실제 Settings Apply/reopen을 endpoint capability와 대조.
- [ ] **BK-WBS-06.12** Windows+Linux의 실제 입력→판정→소리 및 current binary 동작을 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=원본 최소 두 OS 완료 조건; 장치/VM 제공.
  - [ ] **BK-WBS-06.12.01** 현재 Windows binary의 실제 keyboard/HID→binding→판정→출력 소리 전체 경로를 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=로그인 Windows 콘솔·입력·audio endpoint와 current built binary로 전체 playable chart/record/replay를 실행; VM 과거 결과 제외.
  - [ ] **BK-WBS-06.12.02** 현재 Linux binary의 실제 evdev/hidraw→판정→ALSA 소리 전체 경로를 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=/dev/input·/dev/snd 실제 장치와 current built binary로 full song/finite/pause/end·recording 확인; null/CLI smoke 제외.
- [ ] **BK-WBS-06.13** macOS 실제 전체 플레이·입력·오디오 실행을 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=macOS host/device.
  - [ ] **BK-WBS-06.13.01** 현재 macOS binary가 실제 IOHID 입력과 CoreAudio clock으로 full solo playable chart를 완료한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=owned Mac·HAL output·IOHID permission/device와 current binary 필요; actual full song/finite/pause/end·recording 확인.
  - [ ] **BK-WBS-06.13.02** 실제 macOS local cohort와 UI에서 복수 source/member·공유 출력·종료 회수를 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-06.13.01; 필요 근거/다음=같은 Mac에서2인 이상 exact roster·RuntimeGroup capture·GUI 피드백 및 input/output cleanup을 실행.
- [ ] **BK-WBS-06.14** ASIO 실제 driver로 재생·pause/end·종료·buffer 변경을 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=Windows/MSVC/ASIO driver.
  - [ ] **BK-WBS-06.14.01** 실제 설치 ASIO driver의 재생·native clock·pause/finite end crossing을 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=Windows/MSVC/ASIO SDK·신뢰 driver가 필요; 기존 unsupported pause는 명시 거절과 지원 범위를 구별해 기록.
  - [ ] **BK-WBS-06.14.02** 실제 ASIO driver의 buffer 변경·open/start 실패·stop/join·retry 회수를 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-06.14.01; 필요 근거/다음=driver supported granularity/buffer matrix와 실패/취소 경로를 current binary로 실행; retained Source 구현은07.08.07.14와 별도.
- [ ] **BK-WBS-06.15** 추가 출력 API 확장 경계와 지원/미지원 목록을 문서·실제 capability에 맞춘다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=추가 API는 구체 계약/증거 필요; 모든 이름 지원으로 오인 금지.
  - [ ] **BK-WBS-06.15.01** 현재 추가 출력 API 목록을 실제 구현 capability·미지원 오류·배포 문서와 대조한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=plan §11의 추가 API 이름과 platform modules/SettingsHost capability의 지원 표를 만들고 존재만 하는 이름을 지원으로 세지 않음.
  - [ ] **BK-WBS-06.15.02** 추가 API adapter가 기존 exact native clock·owner recovery·static port 확장 경계를 지키는지 확인한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-06.15.01; 필요 근거/다음=현재 지원 선언별 concrete request/applied/native evidence/recovery 계약과 실제 fixture를 연결; 구현 없는 API는 미지원 상태로 문서화.

### 07. 실제 플레이·재생·출력 교체·연속성

기능 연결: BK-017, BK-019, BK-020, BK-021, BK-027, BK-033, BK-035, BK-043. 요구: REQ__native-target-time; selected-policy-network.

- [x] **BK-WBS-07.01** 고정 rate 출력 retirement/recovery에서 whole-owner와 immutable source grid를 보존한다. — 상태=D(검증완료); 선행=-; 근거=E5.
- [x] **BK-WBS-07.02** source/target 분리된 native presentation/start/pause/resume/end 기초 helper를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E1; 실제 launcher 완료는 아래 별도.
- [x] **BK-WBS-07.03** target pause/end rebind에서 source-frame floor와 정확한 target-time floor를 분리한다. — 상태=D(검증완료); 선행=-; 근거=E13 actual mixed-rate/retained-tail/planned basis/frozen source/native ACK/cold finite-end helper 회귀; controller publication은 07.08 별도.
- [x] **BK-WBS-07.04** 기존 출력 lifecycle/controller를 typed target basis로 확장하고 두 번째 runtime을 만들지 않는다. — 상태=D(검증완료); 선행=-; 근거=E15; static generic port·기존 legacy default 회귀·실제 converted owner 및 실패/회수/hold 보존; launcher 연결은 아래 별도.
- [ ] **BK-WBS-07.05** ConvertedAlsaStream을 실제 solo 시작·입력 AudioAuthority·BGM 공급에 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E20 solo, E21 telemetry, E22 local, E23 network target startup 연결·pure 검증; clock-capable endpoint 전체 플레이 및 socket→driver 시작 검증 필요.
  - [ ] **BK-WBS-07.05.01** 실제 Linux solo의 ConvertedAlsaStream을 target startup·AudioAuthority·BGM 공급과 공통 pump에 연결한 소프트웨어 경로를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E20/E21 production solo/telemetry consumer를 current source로 검사; 원본 source grid와 target admission/presentation·gated BGM onset을 비교.
  - [ ] **BK-WBS-07.05.02** network의 committed startup이 socket 합의에서 실제 converted target driver arming/crossing으로 이어진다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E23 shared native_start 경로의 실제 socket→agreement→target projection→driver start/취소·deadline을 같은 owner로 검사.
  - [ ] **BK-WBS-07.05.03** clock-capable ALSA endpoint에서 전체 solo와 network-start 입력/소리/종료를 실행한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.05.01, BK-WBS-07.05.02; 필요 근거/다음=실제 evdev/ALSA 출력·peer가 필요; null과 pure startup fixture는 native full-play 증거가 아님.
- [ ] **BK-WBS-07.06** 같은 converted 출력 owner를 실제 local cohort 경로에 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E22/E23 actual local consumer·null/공유 pump 검증; clock-capable 전체 cohort·member ID/clock/capture/section E2E 및 독립 QA 필요.
  - [ ] **BK-WBS-07.06.01** 실제 local consumer가 같은 converted owner를 distinct-member RuntimeGroup과 공통 cohort pump에서 사용한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E22/E23 actual local consumer·null/공유 pump fixture의 current source에서 member IDs/voice/source identity와 공통 clock을 확인.
  - [ ] **BK-WBS-07.06.02** converted cohort의 pause/section/finite end·capture/record opponent가 원본 member identity를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=native_cohort coordinate/section fixture에 실제 converted state를 연결해 member별 capture/hash/완료 순서 비교.
  - [ ] **BK-WBS-07.06.03** 실제 입력 장치와 clock-capable 출력에서 전체 converted cohort를 독립 검증한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.06.01, BK-WBS-07.06.02; 필요 근거/다음=2인 이상 actual roster·evdev/ALSA endpoint에서 full chart·pause/resume/end·capture 비교와 독립 QA.
- [ ] **BK-WBS-07.07** 실제 recorded playback을 target rate와 source recording grid가 분리된 경로에 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E26 실제 consumer 연결·pause/finite/idle drain·null 회수/원본 cue 검증; clock-capable 전체 replay와 다른 backend/OS·독립 QA 필요.
  - [ ] **BK-WBS-07.07.01** 실제 recorded playback consumer가 target output rate와 source recording grid를 구별해 원본 cue를 재생한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E26 actual replay consumer에서 original cue/frame basis·converted plan/PCM·pause/finite/idle drain을 current source로 확인.
  - [ ] **BK-WBS-07.07.02** recorded replay의 recoverable 실패·partial tail·pause/end가 complete converter와 원본 recording/hash를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=play_replay_bms와 actual converted owner recovery fixture에서 source identity·pending suffix·idle tail를 비교; null 회수는 software tier.
  - [ ] **BK-WBS-07.07.03** clock-capable native endpoint에서 full recorded playback의 실제 presentation/end를 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.07.01, BK-WBS-07.07.02; 필요 근거/다음=실제 Linux/WASAPI/CoreAudio/ASIO supported output에서 full replay 실행; null wall cutoff는 자연종료 아님.
  - [ ] **BK-WBS-07.07.04** 다른 backend/OS의 recorded replay를 동일 source recording과 독립 QA로 대조한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.07.01, BK-WBS-07.07.02; 필요 근거/다음=지원 Windows/macOS와 SDK/driver 환경 필요; original input/judge hash와 target PCM/cleanup 검증.
- [ ] **BK-WBS-07.08** paused output 교체의 open/start/poll/prime/commit을 입력 merger와 원자적으로 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=2개 실제 native progressing 관측; stale publish 상태 보존.
  - [x] **BK-WBS-07.08.01** original-coordinate authority epoch 준비·priming·입력 merger 재검증 기초를 구현하고 검사한다. — 상태=D(검증완료); 선행=-; 근거=E46, 3ecd1bbb authority214 PASS와 기존 input31 PASS; 범위=authority/merger software 경계, 실제 출력 교체는 별도.
  - [x] **BK-WBS-07.08.02** retained Target native presentation과 원래 Mixer creation pin을 보존하는 epoch 기초를 구현하고 검사한다. — 상태=D(검증완료); 선행=BK-WBS-07.08.01; 근거=E47, 9e224d61 successful green67 PASS; 범위=원본 관측 software owner, launcher/device는 별도.
  - [x] **BK-WBS-07.08.03** coordinate pause rebind·frozen adoption·held generation 기초를 실제 Mixer/converter로 검사한다. — 상태=D(검증완료); 선행=BK-WBS-07.08.02; 근거=E48, df70e2e5 pause75 PASS와 포맷 후 집중13 PASS; 범위=원래 source grid/history/pause 보존, pump/UI는 별도.
  - [x] **BK-WBS-07.08.04** sole converted worker의 conditional retirement·거절·취소·회수/retry 경계를 검사한다. — 상태=D(검증완료); 선행=-; 근거=E49, 1463ccfa focused16/full platform387 PASS·ignored2 및 actual ALSA null1 PASS; 범위=software lifecycle, 물리/GUI는 별도.
  - [ ] **BK-WBS-07.08.05** retained Target controller의 open/start/poll/prime/commit을 새 coordinate authority·pause·입력 merger와 원자적으로 연결한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.01, BK-WBS-07.08.02, BK-WBS-07.08.03, BK-WBS-07.08.04; 필요 근거/다음=E51 controller18·실제 publication 할당0회1 및 E55 독립 review/CLI PASS; 실제 native desktop acceptance와 완료 기록 gate는 남음.
    - [x] **BK-WBS-07.08.05.01** retained Target controller의 borrowed atomic publication·stale refusal·allocation0 소프트웨어 범위를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E51 controller18·publication allocation0 및 E55 독립 review/CLI PASS; 범위=실제 controller/authority/pause/merger software, native desktop 실행 아님.
    - [ ] **BK-WBS-07.08.05.02** 실제 native Target 교체의 progressing 관측으로 prime/commit과 acknowledged pause/resume을 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.08.05.01; 필요 근거/다음=clock-capable ALSA endpoint·evdev current binary에서 실제 paused Output 교체/실패/retry를 수행하고 완료 기록 gate를 확인.
  - [ ] **BK-WBS-07.08.06** 새 retained Target 교체를 실제 solo/cohort 공통 pump에 연결하고 acknowledged pause·입력 drain·publication·resume을 검증한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.09.03; 필요 근거/다음=E54 실제 Owner history 경합 연결3 및 E55 독립 review/CLI PASS; 실제 GUI solo/local 시작은 장치 부재로 BLOCKED_ENV이며 native pause/Output/resume/capture와 완료 기록은 남음.
    - [ ] **BK-WBS-07.08.06.01** 실제 공통 solo/cohort pump의 Target Owner history 경합·pause input drain·atomic publication을 software fixture로 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E54 실제 Owner history 경합 연결3 및 E55 독립 review/CLI PASS; 범위=production common pump/Owner/controller와 supplied native observations. 상속된 원래 선행 BK-WBS-07.09.03의 전체 acceptance가 남아 있으므로 tested software 범위를 독립 완료로 승격하지 않음.
    - [ ] **BK-WBS-07.08.06.02** 실제 native solo에서 acknowledged pause→Output 변경→resume이 원본 input/capture를 보존한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.08.06.01; 필요 근거/다음=actual evdev/ALSA clock-capable solo GUI 실행; 실제 시작 장치 부재 BLOCKED_ENV와 완료 기록 gate 유지.
    - [ ] **BK-WBS-07.08.06.03** 실제 native local cohort에서 같은 Target 교체가 member별 capture·소리·input drain을 보존한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.08.06.01; 필요 근거/다음=2인 이상 exact input roster와 native output에서 Output 교체·반복 연습·end를 실행; software 연결3을 hardware로 확대하지 않음.
  - [ ] **BK-WBS-07.08.07** direct SourceFrame/SourceCounter retained 교체의 전체 controller·Owner/UI·공통 pump 및 history/pin 수명을 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E56의 Source child 착수; 모든 하위와 원래 WASAPI/ASIO/CoreAudio 범위를 충족해야 완료. Target 검사를 Source 완료로 확장하지 않는다.
    - [x] **BK-WBS-07.08.07.01** SourceFrame/Counter native epoch·원래 creation pin·exact authority·merger 원자 publication 기초를 연결한다. — 상태=D(검증완료); 선행=-; 근거=E65 독립 DEEP/CLI PASS, native epoch14·authority·controller/공통 pump 연결과 전체 app3258 회귀; 범위=portable 원본 좌표/원자 publication 기초, 실제 Windows 장치 실행은 .05/.10/.11.
    - [x] **BK-WBS-07.08.07.02** Source coordinate pause의 cold proof·새 native descriptor·frozen/startup/history/gap 보존을 연결한다. — 상태=D(검증완료); 선행=-; 근거=E65 독립 DEEP/CLI PASS, pause5·반복 resume/repause·실제 controller/pump 연결4와 creation5/start7·원점 변조 거절; 범위=portable pause 계약, 실제 native ACK/재생은 별도.
    - [x] **BK-WBS-07.08.07.03** NativeOutputState의 pending PCM/history pin과 불확실 admission quarantine을 보존한다. — 상태=D(검증완료); 선행=-; 근거=E65 독립 DEEP/CLI PASS, platform415·실제 pending/history/practice clamp·quarantine 및 controller/Owner Retry의 open 이전 거절; 범위=완전 software 상태 보존, COM 수명은 .05.
    - [x] **BK-WBS-07.08.07.04** 실제 portable WASAPI worker step의 conditional slot·lease·clock-only·실패·무할당 경계를 검사한다. — 상태=D(검증완료); 선행=BK-WBS-07.08.07.03; 근거=E65 독립 DEEP/CLI PASS, production worker18·sizing22·worker 외부 binary81 및 보정된 alloc/realloc/dealloc0; 실제 Exact1ns RED2→GREEN 보존. 범위=portable step, COM 호출 실행은 .05.
    - [ ] **BK-WBS-07.08.07.05** WASAPI shared의 complete Source open/start/Stop/join/recover와 실제 Counter descriptor를 연결한다. — 상태=V(구현·개발검사통과); 선행=BK-WBS-07.08.07.04, BK-WBS-07.08.07.06; 필요 근거/다음=E65 독립 DEEP/CLI의 portable/build-only 범위 PASS; complete MMCSS 정리 오류 보존의 native5개/18subcase는 Windows platform lib/tests typing만 PASS, 미실행. 실제 Windows native acceptance는 endpoint0·실행 환경 부재로 BLOCKED_ENV.
      - [ ] **BK-WBS-07.08.07.05.01** complete WASAPI shared의 open/start/Stop/join/recovery가 SourceCounter descriptor·first-unsent state를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E65 portable/build-only PASS를 기준으로 windows/audio.rs current complete native lifecycle와 MMCSS 진단/error ownership을 audit; 실제 COM 미실행.
      - [ ] **BK-WBS-07.08.07.05.02** cfg-Windows의 native lifecycle 실패 주입5개/18subcase를 같은 production sequence로 실행한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.07.05.01; 필요 근거/다음=Windows platform lib/tests typing만 PASS인 native fixtures를 authentic Windows에서 실행; 원본 operation·MMCSS/Stop/join/cleanup·recovered owner를 검증.
      - [ ] **BK-WBS-07.08.07.05.03** 실제 shared endpoint에서 complete open/start/Stop/회수와 native Counter 관측을 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.08.07.05.01, BK-WBS-07.08.07.05.02; 필요 근거/다음=현재 endpoint0·실행 환경 부재 BLOCKED_ENV; eligible Windows input/audio endpoint에서 actual shared acceptance 수행.
    - [x] **BK-WBS-07.08.07.06** Source conditional port·opaque ticket·created descriptor와 static remix forwarding을 구현한다. — 상태=D(검증완료); 선행=-; 근거=E65 독립 DEEP/CLI PASS, packet3·remix7·browser-only WASM; 범위=원본 행렬/오류/완전 소유권의 정적 전달 계약, 실제 native adapter는 .10.
    - [x] **BK-WBS-07.08.07.07** 기존 replacement lifecycle에 Source controller·conditional ownership·borrowed atomic commit을 연결한다. — 상태=D(검증완료); 선행=BK-WBS-07.08.07.01, BK-WBS-07.08.07.02, BK-WBS-07.08.07.03, BK-WBS-07.08.07.06; 근거=E65 독립 DEEP/CLI PASS, controller21·정확한 startup origin/1ns tamper 거절·연결4·실제 Source publication 할당1; 범위=portable lifecycle/borrowed commit, 실제 native 후보 실행은 별도.
    - [x] **BK-WBS-07.08.07.08** Ticket-generic Source Owner의 관측/query/history와 조건부 publication을 연결한다. — 상태=D(검증완료); 선행=BK-WBS-07.08.07.07; 근거=E65 독립 DEEP/CLI PASS, Owner24·actual UI/공통 pump 연결·publication 검사; typed 오류와 승인 경합·복합 실패를 보존. 범위=generic Owner, 실제 worker와의 Windows 경합은 별도.
    - [x] **BK-WBS-07.08.07.09** Typed Source Output UI의 capability prevalidation·요청/응답·WouldBlock을 연결한다. — 상태=D(검증완료); 선행=BK-WBS-07.08.07.08; 근거=E65 독립 DEEP/CLI PASS, real Owner/controller UI9·sizing 거절24조합·공통 pump 연결; 범위=typed 요청/단일 응답과 live 상태 보존, 실제 Windows GUI 소비자는 .11.
    - [ ] **BK-WBS-07.08.07.10** Canonical WASAPI adapter가 complete Source owner와 원래 Counter packet을 전달한다. — 상태=V(구현·개발검사통과); 선행=BK-WBS-07.08.07.05, BK-WBS-07.08.07.06; 필요 근거/다음=E65 독립 DEEP/CLI에서 portable canonical packet11·전체 app·Windows lib/tests typing PASS; cfgWindows native/classifier5개는 미실행이며 실제 Windows acceptance와 선행 .05가 남음.
      - [ ] **BK-WBS-07.08.07.10.01** canonical Source packet의 portable 검증/전달이 complete owner와 원래 Counter evidence를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E65 독립 DEEP/CLI PASS, canonical packet11·전체 app3258; 범위=portable packet/classification contract, cfg-Windows native 실행은 별도. 상속된 원래 선행 BK-WBS-07.08.07.05의 전체 acceptance가 남아 있으므로 tested software 범위를 독립 완료로 승격하지 않음.
      - [ ] **BK-WBS-07.08.07.10.02** cfg-Windows native adapter/classifier5개와 실제 complete WASAPI 전달을 확인한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.07.10.01; 필요 근거/다음=WASAPI source packet/IO classifier native fixtures를 authentic Windows에서 실행; Source typed 오류를 raw native liveness 오류로 잘못 숨기지 않음.
      - [ ] **BK-WBS-07.08.07.10.03** 실제 WASAPI worker와 adapter가 승인 경합·Stop/recovery 뒤 원본 Counter packet을 전달한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.08.07.10.01, BK-WBS-07.08.07.10.02; 필요 근거/다음=선행 shared complete lifecycle와 실제 Windows endpoint가 필요; `.05` native acceptance 뒤 same-owner history/counter packet 확인.
    - [ ] **BK-WBS-07.08.07.11** Windows solo/local의 실제 SourceCounter coordinate hook과 Legacy/complete static variant를 연결한다. — 상태=V(구현·개발검사통과); 선행=BK-WBS-07.08.07.09, BK-WBS-07.08.07.10; 필요 근거/다음=E65 독립 DEEP/CLI에서 Windows bin72·Source UI9(거절24조합 포함)·전체 desktop app3258 및 Windows app typing PASS; cfgWindows mapper/COM 실제 실행과 선행 .10이 남음.
      - [ ] **BK-WBS-07.08.07.11.01** Windows solo/local SourceCounter hook과 Legacy/complete static variant의 portable 조립을 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E65 독립 DEEP/CLI PASS, Windows bin72·typed Source UI9/24거절조합·전체 desktop app3258; 범위=portable composition 및 Windows Rust typing. 상속된 원래 선행 BK-WBS-07.08.07.10의 전체 acceptance가 남아 있으므로 tested software 범위를 독립 완료로 승격하지 않음.
      - [ ] **BK-WBS-07.08.07.11.02** cfg-Windows mapper/actual COM 관측과 source startup 선택을 실행한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.07.11.01; 필요 근거/다음=Windows Source hook mapper를 authentic Windows로 실행하고 actual Counter descriptor·initial history·retained opt-in과 legacy 호환을 확인.
      - [ ] **BK-WBS-07.08.07.11.03** 실제 Windows solo/local current binary의 complete retained 시작과 typed Output 소비를 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.08.07.11.01, BK-WBS-07.08.07.11.02; 필요 근거/다음=선행 native adapter/endpoint acceptance 및 Raw Input/audio 환경 필요; actual GUI/CLI solo/local play로 source identity·capture 확인.
    - [ ] **BK-WBS-07.08.07.12** Source Frame/Counter 공통 solo/cohort pump에서 교체·resume·loop/scrub·원본 capture/PCM 보존을 검증한다. — 상태=V(구현·개발검사통과); 선행=BK-WBS-07.08.07.09, BK-WBS-07.08.07.11; 필요 근거/다음=E65 독립 DEEP/CLI에서 실제 연결4·전체 desktop app3258 PASS, PCM 회계·종료 silence·원본 capture codec/hash·member/attempt·조건부 경합 검사; 실제 Windows acceptance와 선행 .11이 남음.
      - [ ] **BK-WBS-07.08.07.12.01** Source Frame/Counter 공통 solo/cohort pump의 loop/scrub·PCM·capture·member/attempt 보존을 실제 software 연결로 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E65 독립 DEEP/CLI PASS, actual pump 연결4·전체 app3258, PCM 회계·silence·capture codec/hash·conditional races; 범위=supplied native evidence software. 상속된 원래 선행 BK-WBS-07.08.07.11의 전체 acceptance가 남아 있으므로 tested software 범위를 독립 완료로 승격하지 않음.
      - [ ] **BK-WBS-07.08.07.12.02** 실제 Windows native Source 교체/반복 연습/resume/end에서 원본 PCM·입력/capture를 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.08.07.12.01; 필요 근거/다음=선행 Source startup과 native endpoint/input 환경; actual solo/local pause/replace/resume·loop/fractional scrub·finite end 수행.
    - [ ] **BK-WBS-07.08.07.13** WASAPI exclusive retained Source 교체의 실제 mode/geometry·history·cleanup 및 pause/resume을 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E66의4개 실행 단위와 원래 native acceptance를 모두 충족해야 완료; shared 검사를 exclusive 완료로 확장하지 않는다.
      - [x] **BK-WBS-07.08.07.13.01** exclusive의 known Exact/actual geometry·전체 native buffer lease·whole-multiple PCM 복구와 conditional worker를 검사한다. — 상태=D(검증완료); 선행=-; 근거=E70 실제 독립 DEEP/CLI PASS, production worker37·sizing22·외부 binary100과 보정된 heap0; shared cold-helper3조합·원본 PCM/상태·오류/경합 보존. 범위=portable production step/geometry; COM/native 실행은 .13.02/.13.04.
      - [ ] **BK-WBS-07.08.07.13.02** complete exclusive open/start/Stop/join/recovery에 actual native buffer·Counter/history·정리 오류를 연결한다. — 상태=V(구현·개발검사통과); 선행=BK-WBS-07.08.07.13.01; 필요 근거/다음=E70 독립 DEEP/CLI의 portable/build-only 범위 PASS; actual geometry/preparation 순서와 Ready/event dispatch·complete cleanup 연결, Windows lib/tests typing0. 새 native9개는 미실행이며 실제 Windows acceptance는 endpoint0·실행 환경 부재로 BLOCKED_ENV.
        - [ ] **BK-WBS-07.08.07.13.02.01** exclusive complete native lifecycle가 actual buffer geometry·Counter/history·MMCSS cleanup diagnostics를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E70 portable/build-only PASS, Ready/event dispatch·geometry/preparation·cleanup 연결을 current source로 audit; typing0을 COM 실행으로 세지 않음.
        - [ ] **BK-WBS-07.08.07.13.02.02** cfg-Windows exclusive native9개 실패 주입을 production lifecycle에서 실행한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.07.13.02.01; 필요 근거/다음=새 native9개는 미실행; exact buffer mismatch/prefill/start/Stop/join/cleanup과 complete recovered state를 authentic Windows에서 검증.
        - [ ] **BK-WBS-07.08.07.13.02.03** 실제 exclusive endpoint의 complete open/start/회수·actual Counter를 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.08.07.13.02.01, BK-WBS-07.08.07.13.02.02; 필요 근거/다음=현재 endpoint0·native 실행 환경 부재 BLOCKED_ENV; 실제 Windows exclusive 장치 acceptance 수행.
      - [ ] **BK-WBS-07.08.07.13.03** Windows retained exclusive startup·same-family Output UI와 기존 Owner/solo/local 소비 경로를 연결한다. — 상태=V(구현·개발검사통과); 선행=BK-WBS-07.08.07.13.02; 필요 근거/다음=E70 독립 DEEP/CLI에서 Windows bin79(기본 device RED2→GREEN2 포함)·전체 app3258/bins624·공통 Owner/UI/pump·Windows app typing0 PASS; 선행 native 실행과 실제 consumer acceptance가 남음.
        - [ ] **BK-WBS-07.08.07.13.03.01** exclusive retained startup·same-family typed UI와 기존 Owner/solo/local의 portable composition을 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E70 독립 DEEP/CLI PASS, Windows bin79·전체 app3258/bins624·common Owner/UI/pump·Windows app typing0; 범위=software 조립, native 실행 아님. 상속된 원래 선행 BK-WBS-07.08.07.13.02의 전체 acceptance가 남아 있으므로 tested software 범위를 독립 완료로 승격하지 않음.
        - [ ] **BK-WBS-07.08.07.13.03.02** 실제 exclusive consumer의 startup/default device·same-family Output Apply/resume을 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.08.07.13.03.01; 필요 근거/다음=선행 exclusive complete native acceptance와 actual Windows input/audio/GUI 필요; 실제 device blank/default와 buffer 변경을 실행.
      - [ ] **BK-WBS-07.08.07.13.04** 실제 Windows exclusive 출력에서 pause/replace/resume·반복 연습·입력/capture/PCM·실패 정리를 검증한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.08.07.13.03; 필요 근거/다음=E70 새 소유 VM QA의 실제 COM Render/AllStates 열거/GetCount HRESULT0·endpoint0, 로그인 콘솔/현재 player 부재. 초기Off 복구 완료; eligible Windows에서 실제 exclusive 장치 acceptance 수행.
        - [ ] **BK-WBS-07.08.07.13.04.01** 실제 exclusive solo에서 pause/replace/resume·반복 practice와 원본 PCM/input/capture를 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=eligible Windows exclusive endpoint·Raw Input/current player 필요; E70 새 VM endpoint0·로그인 콘솔 부재 blocker 유지.
        - [ ] **BK-WBS-07.08.07.13.04.02** 실제 exclusive local cohort의 member/attempt·종료 silence·교체 실패 정리를 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=2인 이상 exact roster와 exclusive 출력에서 failure/retry/end·capture hash와 Stop/join/MMCSS 진단을 확인; 실제 native9개 실행 결과 포함.
    - [ ] **BK-WBS-07.08.07.14** ASIO SourceFrame retained 교체의 callback conditional retirement·완전 상태·Owner/UI/pump를 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=공통 Frame 기초와 실제 ASIO adapter는 별도; SDK/build 라이선스 조건·형/장치 실행·실패/재시도 검사 필요.
      - [ ] **BK-WBS-07.08.07.14.01** ASIO sole callback의 actual SourceFrame suffix·geometry에 대해 conditional retirement와 취소/승인 경합을 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=portable production callback20·실제 C++ dispatch6을 실행했다. sole native gate의 cfg 형 검사와 원본 prefix/geometry/nonce 회귀와 독립 소프트웨어 review/QA PASS가 있다. authentic SDK/driver 증거는 남는다.
      - [ ] **BK-WBS-07.08.07.14.02** 실제 heterogeneous planar 형식에 맞는 ASIO complete 출력 소유자를 prepare/start/Stop/join/recovery와 원래 SourceFrame/history identity에 연결한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.07.14.01; 필요 근거/다음=a56e039b의 complete NativeOutputState/AsioPlanarLayout·실패 복구·history 없는 pending 사전 거절을 구현했다. native lifecycle21은 Rust-only typed-only이며 반복 거절/빈 tail 정상 경로가 포함된다. 독립 소프트웨어 review/QA는 PASS했으며 실제 SDK/driver 검사 필요.
      - [ ] **BK-WBS-07.08.07.14.03** ASIO canonical adapter가 opaque native ticket·original frame/interval packet·complete recovery를 전달한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.07.14.02; 필요 근거/다음=90f3e5ec의 static complete adapter·출력별 bounds·typed 오류/원본 interval cache를 연결했다. adapter17개는 Windows Rust-only typed-only다. 독립 소프트웨어 review/QA는 PASS했으며 native 실행 필요.
      - [ ] **BK-WBS-07.08.07.14.04** ASIO typed Source Output UI가 실제 capability·요청/응답·WouldBlock과 cold refusal를 연결한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.07.14.03; 필요 근거/다음=d53b777d·8ee68fb1의 common Source UI/Owner·출력별 bounds·실제 HWND guard와 typed setup failure를 연결했다. host Windows bin94/0(새 portable15 포함), Windows Rust-only 형 검사 exit0. 독립 소프트웨어 review/QA는 PASS했으며 실제 GUI/driver 확인 필요.
      - [ ] **BK-WBS-07.08.07.14.05** Windows ASIO solo의 retained SourceFrame startup/coordinate hook을 기존 common pump에 연결한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.07.14.03, BK-WBS-07.08.07.14.04; 필요 근거/다음=d53b777d의 AsioComplete static variant가 원래 common solo Source pump를 사용한다. host planner/cold PCM·history·SDK-disabled 회귀와 gated composition 형 검사를 확인했다. 독립 소프트웨어 review/QA는 PASS했으며 실제 Windows solo 재생은 남는다.
      - [ ] **BK-WBS-07.08.07.14.06** Windows ASIO local cohort의 retained SourceFrame startup/공통 pump와 member별 capture를 연결한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.07.14.03, BK-WBS-07.08.07.14.04; 필요 근거/다음=d53b777d의 AsioComplete owner를 common RuntimeGroup/Source pump에 연결하고 typed 오류·모든 capture 시도를 유지한다. shared finalizer 회귀는 실행했지만 native local caller는 static-only다. 독립 소프트웨어 review/QA는 PASS했으며 실제 Windows cohort 실패/재개는 남는다.
      - [ ] **BK-WBS-07.08.07.14.07** ASIO production callback·adapter/UI·solo/local software를 독립 검토와 필수 QA로 확인한다. — 상태=W(구현·연동중); 선행=BK-WBS-07.08.07.14.01, BK-WBS-07.08.07.14.02, BK-WBS-07.08.07.14.03, BK-WBS-07.08.07.14.04, BK-WBS-07.08.07.14.05, BK-WBS-07.08.07.14.06; 필요 근거/다음=최종 DEEP code/security 실제 PASS0 뒤 독립 QA 실제 PASS: core749·platform lib507·app lib3319/bins670·allocator1·CLI19 및 foreign typing/C++/WBS 확인. hook-owned 최종 완료 gate는 별도이며 native acceptance로 확대하지 않음.
      - [ ] **BK-WBS-07.08.07.14.08** 실제 MSVC/ASIO SDK·설치 driver에서 retained pause/replace/resume·drain/음향을 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.08.07.14.07; 필요 근거/다음=Windows/MSVC/허용된 ASIO SDK와 신뢰 installed driver 필요; shared WASAPI와 foreign typing을 ASIO 근거로 확대하지 않음.
    - [ ] **BK-WBS-07.08.07.15** CoreAudio retained Source 교체의 원래 native 좌표·callback/회수·Owner/UI/pump를 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 근거=E75 사전 REQ/ADR ef99f3a3·독립 계획 검토와 병렬 기반 구현 시작; 필요 근거/다음=실제 complete callback·회수·backend/UI·solo/local 구현·소프트웨어 검증 및 Mac/HAL 장치 acceptance; 현재 owned Mac 없음·macos-host 누락이며 foreign typing은 장치 근거가 아님.
      현재 소프트웨어 연결은 완료했고 frozen2a7313c6 독립 code/security review와 CLI QA를 통과했다. 실제 native lifecycle/IOHID·음향은 .15.12에 남긴다.
      - [x] **BK-WBS-07.08.07.15.01** 공통 Source retirement CAS가 actual suffix·geometry·checked nonce·busy/cancel/sticky authorization을 무할당으로 보존한다. — 상태=D(검증완료); 선행=-; 근거=8b9a63e9, E75 foundation-platform.log에서 audio::source_output_retirement fixtures10/0 확인; 전체 platform450/0/ignored2 내부 중복이며 native drain 증거 아님.
      - [x] **BK-WBS-07.08.07.15.02** CoreAudio complete cold 준비의 선택적 history capacity 성장이 원본 span/identity/revision·pending PCM을 보존한다. — 상태=D(검증완료); 선행=-; 근거=f8afbef9, E75 ac010-core.log history14/0 및 foundation-platform.log output_state21/0(전체450에 포함); allocation refusal/64→128/immutable limit·legacy helper 보존.
      - [x] **BK-WBS-07.08.07.15.03** portable production CoreAudio callback이 승인 선행·전체 channel 검증·original pending prefix·first-unsent admission·history/practice pin과 zero heap을 검증한다. — 상태=D(검증완료); 선행=BK-WBS-07.08.07.15.01, BK-WBS-07.08.07.15.02; 근거=40b8821b, E75 ac003-callback-fixed.log19/0 확인; 최초17/2의 잘못된 interval 분할 기대를 원본 covering span·실제 pause compaction oracle로 교정. 실제 HAL callback 실행은 아님.
      - [ ] **BK-WBS-07.08.07.15.04** 하나의 CoreAudio Legacy/Complete native owner에서 complete open/start/Stop/unregister/drain/recovery·opaque tickets·first-unsent telemetry를 연결한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.07.15.01, BK-WBS-07.08.07.15.02, BK-WBS-07.08.07.15.03; 근거=E75 macOS C-stub 전체 lib/tests/macOS bin typing exit0, 실제 RenderOwner·weak ticket·geometry·start-attempt·drain 구현; 필요 근거/다음=actual Mac/HAL lifecycle 실행; 정적 native ownership 검토는 통과했다. C-stub은 native 행동 증거가 아니다. 현재=b8f7494f의 승인/failed Start 후 live Source fence·actual maximum clock qualification에 더해32c5b78d에서 실제 callback 크기의 converter 준비와 production 등록 sequence 주입을 연결했다. host platform473/0/ignored2·macOS platform/app lib/tests/bin typing exit0; 독립 정식 code/security review·소프트웨어 CLI QA는 통과했고 실제 HAL 실행은 남는다.
      - [ ] **BK-WBS-07.08.07.15.05** 같은 CoreAudio native lifecycle의 정적 operation 주입으로 failed-start callback·cleanup retry·ticket/geometry·complete ownership 실패 fixture를 완성한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.07.15.04; 필요 근거/다음=retained_source_fixtures.rs의 신규27개와 open_failure의 신규1개, 보존24개로 native audio fixture52개를 작성했다. macOS C-stub lib/tests typing은 exit0이며 actual 실행은 없다. 같은 production sequence의 failed Start/Stop/listener removal/IOProc destroy·original/cleanup error·unique owner 검증을 actual Mac에서 실행한다. 현재=승인 후 관측/요청·읽기 중 authorization 경합4개, create/null-ID·모든 listener 등록 실패·부분 cleanup 재시도4개와 presentation maximum6개를 보강했다. macOS platform/app lib/tests/bin typing exit0(21.35초); native fixture 실제 실행은 아직 없다.
      - [ ] **BK-WBS-07.08.07.15.06** CoreAudio canonical backend가 complete owner·원래 Mach/sample/flags SourceFrame packet·conditional tickets와 recovery를 전달한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.07.15.04, BK-WBS-07.08.07.15.05; 필요 근거/다음=app/src/gameplay/output/adapters/coreaudio.rs와 coreaudio_source_packet_fixtures.rs에 existing Source ports 구현; 현재 legacy Mixer backend를 complete software state로 flatten하지 않음. 현재=complete lifecycle/remix/Source/history/conditional 기존 port 연결, 원래 matrix/capacity/오류 보존과 기존 packet 검사 공유를 작성했다. macOS app lib/tests/bin typing exit0; 신규 classifier3개를 더한 app fixture24개는 cfg-Mac typed-only이며 실제 native 실행은 남는다. 후속=2c14803a 독립 formal code review FAIL2로 정상 future native 관측 대기와 history/authorization 경합의 shared port·typed queue·narrow classifier를 AC012에서 수정했다. 새 host 회귀19개를 포함한 전체 app library3279/0/ignored8·최종 macOS platform/app lib/tests/bin typing exit0·Rust40 format/diff를 확인했다. cfg-Mac24개는 typed-only이며 frozen2a7313c6 두 번째 정식 code/security review와 독립 소프트웨어 CLI QA는 통과했다. 실제 native 실행은 남는다.
      - [ ] **BK-WBS-07.08.07.15.07** CoreAudio typed Source Output UI가 실제 maximum/format/geometry capability·요청/응답·WouldBlock·승인 전 publication 금지를 연결한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.07.15.06; 필요 근거/다음=coreaudio_ui.rs의 legacy GameplayOutputOwner 조립을 기존 common Source Owner/UI에 static variant로 연결; actual request prevalidation·one reply/capability refresh 및 variable/fixed suffix 거절 fixture. 현재=83b50487에 static Legacy/Complete facade·actual maximum capability·기존 coordinate bridge 연결과 신규7개 fixture를 커밋했다. macOS lib/tests/bin typing exit0; host 공통 retained_source_requests9/0 실행. 최종 독립 소프트웨어 QA를 통과했으며 신규 cfg-Mac UI fixture의 실제 실행은 남는다.
      - [ ] **BK-WBS-07.08.07.15.08** macOS solo의 retained 시작과 coordinate hook이 complete CoreAudio owner·공통 Source pump를 소비한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.07.15.06, BK-WBS-07.08.07.15.07; 필요 근거/다음=app/src/bin/macos_bms.rs의 current legacy CoreAudioStream::open 시작을 retained opt-in complete variant로 연결; original creation pin·two actual progressing coordinates·pause/resume/loop/scrub·capture 보존. 현재=9e68bbb4에 complete retained 시작·조건부 Source/pause UI hook·공유 cold config를 연결했다. host consumer8개 통과와 macOS typing exit0; 최종 독립 소프트웨어 QA는 통과했고 실제 IOHID/HAL forwarding·연결 여정은 남는다.
      - [ ] **BK-WBS-07.08.07.15.09** macOS local cohort가 같은 complete CoreAudio owner·typed Source UI·공통 cohort pump를 소비한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.07.15.06, BK-WBS-07.08.07.15.07; 필요 근거/다음=app/src/bin/macos_bms/local.rs의 scalar owner를 retained complete static variant에 연결; 실제 RuntimeGroup member/voice/source/capture·section/end·PCM 보존. 현재=9e68bbb4에서 같은 complete owner·독립 RuntimeGroup/capture를 연결하고 초기 open 실패도 원래 typed error를 보존하며 공통 all-member finalizer로 보낸다. macOS typing exit0; 최종 독립 소프트웨어 QA는 통과했고 실제 IOHID/HAL 여정은 남는다.
      - [x] **BK-WBS-07.08.07.15.10** 합쳐진 CoreAudio complete callback/native ownership/backend/UI/solo/local candidate를 독립 code·조건부 security review로 확인한다. — 상태=D(검증완료); 선행=BK-WBS-07.08.07.15.03, BK-WBS-07.08.07.15.04, BK-WBS-07.08.07.15.05, BK-WBS-07.08.07.15.06, BK-WBS-07.08.07.15.07, BK-WBS-07.08.07.15.08, BK-WBS-07.08.07.15.09; 근거=E75, frozen2a7313c6의 독립 정식 code_review_coreaudio_14036·security_review_coreaudio_14036가 각각 PASS0. 앞선 FAIL2는 edc8294e에서 수정하고 실제 회귀를 확인했다. 범위=전체 candidate의 code/security 검토이며 실제 HAL 행동 검증이나 owning task close가 아니다. 이전 검사 기준=source freeze 후 독립 DEEP review가 native unsafe buffers/overlap·first-unsent basis·CAS publication·cleanup ordering·legacy compatibility를 검사하고 모든 FIX_NOW 해결; 개발 test PASS는 review 아님.
      - [ ] **BK-WBS-07.08.07.15.11** 리뷰 후 CoreAudio portable/runtime/adapter/UI/solo/local 회귀·macOS typing·최종 문서 audit를 독립 QA로 확인한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.07.15.10; 근거=E75, 정식 리뷰 PASS 뒤 독립 qa_cli_coreaudio_14036가 software scope PASS. core749·platform473·format23·app3279·bins640·외부 allocation120/1·WASM/Windows/macOS typing·CLI17/17을 확인했다. cfg-Mac는 typed-only이며 외부/consumer 수의 중복 합산 금지. 필요 근거/다음=ordered hook attestation을 갖춘 owning verification/close. 독립 문서 재검토 PASS0 뒤 한 번 task_verify를 실행했으나 receipt stream 없음으로 PENDING이라 V 유지; 실제 리뷰/QA PASS와 task close는 별개다. 이전 검사 기준=cargo test -p beatkernel-platform --lib --locked coreaudio_source_callback 및 전체 core/platform/app·macOS actual/cfg test 실행 범위를 분리; frozen source/hash·원본 stdout/exit·alloc calibration·legacy 회귀와 문서/owning gate 확인.
      - [ ] **BK-WBS-07.08.07.15.12** 실제 Mac/HAL/IOHID에서 complete callback·registration/start/stop/drain·두 native progressing 관측·retained solo/local·음향 acceptance를 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.08.07.15.11; 필요 근거/다음=device broker owned Mac 없음, macos-vm missing macos-host/providers 없음; eligible owned Mac 확보 후 actual HAL/IOHID·실제 Output UI/loop/scrub/resume/end·PCM/capture/음향 확인. portable/C-stub은 장치 근거 아님.
  - [ ] **BK-WBS-07.08.08** 기존 scalar 교체와 finite-end ACK·held tail·취소/실패 경로가 새 retained 교체 변경 후에도 보존되는지 재검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=기존 구현/fixture는 존재하나 current 합쳐진 후보의 regression·실제 native crossing·end/drain/owner cleanup 및 독립 확인 필요.
    - [ ] **BK-WBS-07.08.08.01** 새 retained 변경 뒤 기존 scalar/legacy/remixed 교체와 owner recovery를 current 합쳐진 candidate로 재검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=기존 lifecycle/controller/adapter fixture를 source freeze 후 실행; complete state의 into_mixer 거절이 legacy remixed 복구를 깨지 않는지 확인.
    - [ ] **BK-WBS-07.08.08.02** finite-end native ACK·held tail·pause/resume·취소/실패·retry의 원본 순서와 PCM 보존을 재검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=native_end/live_pause/retained replacement fixtures의 actual pending/held/finite seal·admission quarantine를 current source로 실행.
    - [ ] **BK-WBS-07.08.08.03** 실제 native crossing/end/drain/cleanup에서 scalar 호환을 독립 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.08.08.01, BK-WBS-07.08.08.02; 필요 근거/다음=지원 backend 실제 장치에서 기존 scalar start/pause/end/replace·failure cleanup을 실행; portable PASS만으로 original native 회귀 완료 금지.
  - [ ] **BK-WBS-07.08.09** Windows/macOS와 WASAPI↔ASIO 등 foreign/cross-backend retained 교체의 현재 연결·applied format·누락 경계를 감사한다. — 상태=U(최신현황감사필요); 선행=-; 필요 근거/다음=Target Linux/foreign stub typing을 전체 native 적용으로 간주하지 않음; 원래 backend/epoch/latency/history/cleanup 기준과 실제 SDK·host 실행 범위를 확인.
    - [ ] **BK-WBS-07.08.09.01** Windows WASAPI shared/exclusive·ASIO retained Source의 current adapter/owner/UI 연결 범위를 감사한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=source survey로 complete/legacy mode·Frame/Counter descriptor·SDK gating·unsupported cross-family refusal 표를 확정; Source 기초를 ASIO 완료로 세지 않음.
    - [ ] **BK-WBS-07.08.09.02** macOS CoreAudio retained Source의 actual callback/native owner/UI/solo/local 및 미연결 seam을 감사한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=BK-WBS-07.08.07.15의12단위와 current macos files/actual runner inventory를 대조; macOS C-stub typing/portable step/native HAL 증거를 분리.
    - [ ] **BK-WBS-07.08.09.03** foreign/cross-backend 교체의 applied format·epoch·latency·history·cleanup와 실제 host/SDK 증거의 누락을 명시한다. — 상태=U(현황감사필요); 선행=BK-WBS-07.08.09.01, BK-WBS-07.08.09.02; 필요 근거/다음=WASAPI↔ASIO 등 각 pair의 actual request/capability/publication/recovery 경로와 환경을 audit; Target Linux 결과를 전체 foreign 적용으로 확장하지 않음.
  - [ ] **BK-WBS-07.08.10** 실제 native retained 출력 교체에서 두 progressing 관측·입력 provenance·paused stale 보존·음향 alignment를 검증한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=실제 evdev/IOHID/Raw Input·native clock·출력 장치/ASIO SDK·물리 loopback; null/software/stub는 대체 근거가 아님.
    - [ ] **BK-WBS-07.08.10.01** 실제 native 교체가 두 fresh progressing 원본 좌표와 acknowledged-paused stale rejection을 증명한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=지원 backend별 실제 native clock/device에서 priming·publication·stale/tamper 거절 전후 owner/epoch/input binding trace 필요.
    - [ ] **BK-WBS-07.08.10.02** 실제 Raw Input/evdev/IOHID acquired prefix와 capture provenance가 교체·resume을 지나 보존된다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.08.10.01; 필요 근거/다음=실제 입력 장치와 native output에서 Down/Up/held/release·source sequence/domain·capture/live hash 비교.
    - [ ] **BK-WBS-07.08.10.03** 실제 교체의 소리·buffer/backend/rate alignment와 종료/cleanup을 loopback으로 검증한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.08.10.01, BK-WBS-07.08.10.02; 필요 근거/다음=물리 loopback·clock-capable endpoint 및 ASIO SDK/driver 환경 필요; null/software/stub은 물리 continuity 증거 아님.
- [ ] **BK-WBS-07.09** rate/buffer/channel/encoding 변경을 실제 설정 요청·capability·applied 결과까지 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E19 target rate/buffer/period/matrix UI bridge 검증; 실제 launcher/encoding 제어·pending PCM 해석 보존은 계속 필요.
  - [x] **BK-WBS-07.09.01** Linux encoding 설정·profile·CLI 및 실제 desktop Settings 편집/거절/Apply·재시작의 bounded software 경로를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E50, 18d9c414 독립 code/CLI QA·실제 Settings/profile·129-row 경계; 범위=Linux cold 설정, acknowledged-paused Output/물리는 별도.
  - [x] **BK-WBS-07.09.02** Linux encoding ALSA request/capability/applied·solo/local startup·recorded replay와 pending 해석 거절/recovery를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E50, 실제 production adapter/cold consumer·독립 app QA 및 두 opted ALSA null 검사; 범위=software encoding, retained Apply/물리/foreign는 별도.
  - [ ] **BK-WBS-07.09.03** 새 retained Target typed Output Owner/UI 요청·응답·capability refresh를 교체 controller에 연결한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-07.08.05; 필요 근거/다음=E54 경합 수정 및 E55 독립 review/CLI PASS, 실제 Settings Apply/reopen 확인; paused Output의 실제 applied capability·resume 및 완료 기록 gate는 남음.
    - [ ] **BK-WBS-07.09.03.01** retained Target typed UI의 request/response·capability refresh·경합 보존을 actual Owner/controller에서 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E54 요청/관측25·actual Owner history 경합 연결3, E55 독립 review/CLI PASS와 실제 Settings Apply/reopen; 범위=software UI/control wiring. 상속된 원래 선행 BK-WBS-07.08.05의 전체 acceptance가 남아 있으므로 tested software 범위를 독립 완료로 승격하지 않음.
    - [ ] **BK-WBS-07.09.03.02** 실제 acknowledged-paused Output에서 applied capability·resume과 완료 기록 gate를 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.09.03.01; 필요 근거/다음=실제 native admitted playback·evdev/clock-capable output 필요; current Output Apply/reopen 및 native resume/원본 capture를 확인.
  - [ ] **BK-WBS-07.09.04** 실제 acknowledged-paused Output GUI에서 retained 설정 Apply/거절·capability refresh·Back/reopen을 실행한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=E50 evdev 부재 BLOCKED_ENV; admitted native play/paused ACK 뒤 실제 입력과 current binary로 검사, Settings/null fixture 결과로 대체하지 않음.
    - [ ] **BK-WBS-07.09.04.01** 실제 paused Output GUI의 Apply/거절이 actual applied 설정·capability refresh와 단일 응답으로 이어진다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=E50 evdev 부재 BLOCKED_ENV; 실제 입력/native pause ACK 후 current binary에서 valid/invalid buffer/matrix/encoding 요청 실행.
    - [ ] **BK-WBS-07.09.04.02** 실제 paused Output의 Back/reopen·재시도·resume이 원본 owner/epoch/capture를 보존한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.09.04.01; 필요 근거/다음=같은 admitted play/GUI에서 pending/back/reopen·거절 후 재편집·성공 후 resume을 실행; Settings/null fixture로 대체하지 않음.
  - [ ] **BK-WBS-07.09.05** 남은 모든 지원 플랫폼·backend의 rate/buffer/channel/encoding 요청·capability·실제 applied 및 pending PCM 해석 범위를 감사·연결한다. — 상태=U(최신현황감사필요); 선행=-; 필요 근거/다음=원래07.09 전체 기준·E19 target bridge와 미지원 선택의 명시 거절 유지; Source/foreign/실제 launcher·장치 조합을 확인하고 missing seam을 별도 bounded 단위로 갱신.
    - [ ] **BK-WBS-07.09.05.01** Source backend별 rate/buffer/channel/encoding 요청·capability·actual applied·pending 해석의 current 지원 범위를 감사한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=WASAPI shared/exclusive/ASIO/CoreAudio complete/legacy의 actual mapper·request·applied type을 표로 대조; unsupported Source rate/encoding을 silent fallback으로 지우지 않음.
    - [ ] **BK-WBS-07.09.05.02** foreign launcher와 recorded replay의 설정 소비·pending PCM 해석 보존 및 missing seam을 bounded 실행 단위에 연결한다. — 상태=U(현황감사필요); 선행=BK-WBS-07.09.05.01; 필요 근거/다음=각 OS solo/local/replay consumer의 CLI→owner/native applied와 format 변경 시 원본 suffix 보존/거절을 조사; 원래07.09 기준과 E19 bridge를 유지.
- [ ] **BK-WBS-07.10** startup/pause/resume/finite end ACK가 lookahead/held-release가 아니라 native crossing을 기다린다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E16 공개 audio pump의 pause/resume/held 순서 검증; 실제 launcher owner→worker→native 관측 E2E 및 finite end는 계속 필요.
  - [ ] **BK-WBS-07.10.01** native startup ACK가 generated lookahead/held release 대신 actual presentation crossing을 기다린다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=native_start/startup Source/Target 실제 consumer와 E16 shared pump 검사를 current source로 확인; created descriptor/prefill을 진행으로 세지 않음.
  - [ ] **BK-WBS-07.10.02** native pause/resume ACK가 실제 adopted boundary·held mapping·후속 native crossing을 요구한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=live_pause/native_audio_presentation·공통 solo/cohort에서 actual worker→owner의 pause/resume packet을 검사; cached held report/host timer 대체 거절.
  - [ ] **BK-WBS-07.10.03** finite-end ACK가 native endpoint·원본 input drain·command retirement를 모두 기다린다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=native_end/finite seal와 실제 launcher에서 lookahead/end marker·expired command와 original acquired prefix를 구별해 full idle tail/zero fence를 검사.
  - [ ] **BK-WBS-07.10.04** 실제 출력 장치의 startup/pause/resume/finite crossing과 cleanup을 end-to-end 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.10.01, BK-WBS-07.10.02, BK-WBS-07.10.03; 필요 근거/다음=eligible native endpoint/input/current binary에서 crossing trace·ACK 지연·Stop/drain을 기록; source helper/null report는 native proof 아님.
- [ ] **BK-WBS-07.11** WASAPI↔ASIO 등 cross-backend 교체의 현재 소스 범위와 missing seam을 audit한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=WindowsRequest/공통 owner는 양쪽 open과 원래 Mixer·native clock 관측을 이미 지원; 현재 백엔드만 선택하던 Windows UI mapper와 output-only schema를 [대상 선택 계약](REQ__windows-output-backend-switch.md)에 따라 연결 중. 실제 SDK/장치 전환·latency/cleanup 검증은 남음.
  - [ ] **BK-WBS-07.11.01** Windows Output target planner가 backend를 먼저 선택하고 WASAPI endpoint/ASIO CLSID·inactive fields·default를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=REQ__windows-output-backend-switch 개발 Windows bin59·schema2·common output103을 current source로 실행; target admission과 current source channel width 보존 확인.
  - [ ] **BK-WBS-07.11.02** shared Windows owner의 WASAPI↔ASIO open/clock epoch·Mixer 회수·typed capability refresh를 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=WindowsRequest·WindowsOutputUi·existing recoverable/pending cleanup을 실제 common owner로 검사; format/open failure의 original owner·matrix·domain과 backend fallback 금지.
  - [ ] **BK-WBS-07.11.03** 실제 SDK/driver에서 cross-backend latency·cleanup·pause/교체/resume을 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.11.01, BK-WBS-07.11.02; 필요 근거/다음=Windows/MSVC/ASIO SDK·installed ASIO driver와 WASAPI endpoint 필요; 기존 scalar 전환과 retained Source 지원 거절은 구별.
- [ ] **BK-WBS-07.12** unknown/stale/wrong-domain 관측·취소·실패·재시도가 owner를 유실하지 않는다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=모든 실제 launcher/replacement 회귀.
  - [ ] **BK-WBS-07.12.01** unknown/stale/wrong-domain native packet과 query/history 오류가 current owner·epoch·accepted frontier를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=Source/Target Owner와 adapter classifier·atomic commit의 current error matrices를 실행; typed coordinate/history 오류를 retirement 권한으로 숨기지 않음.
  - [ ] **BK-WBS-07.12.02** 취소/승인 경합·open/start/Stop/join/cleanup 실패·Retry가 같은 unique complete owner를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=conditional ticket·uncertain admission quarantine·pending native owner를 actual common controller/launcher fixture에서 대조; error/cleanup을 덮어쓰지 않음.
  - [ ] **BK-WBS-07.12.03** 모든 실제 launcher에서 native 오류/분리·재시도가 자원/PCM을 유실하지 않는지 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.12.01, BK-WBS-07.12.02; 필요 근거/다음=Windows/Linux/macOS actual backend/input 환경과 failure/취소 injection 필요; cfg typing/portable state만으로 native cleanup 완료 금지.
- [ ] **BK-WBS-07.13** 모든 OS solo/local launchers가 실제 선택 class/gauge 정책 검증을 통해 network를 준비한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E1/E8 일부; 현재 Windows/macOS parser·foreign typing 재검사.
  - [ ] **BK-WBS-07.13.01** 공통 solo/local policy admission이 실제 selected class/gauge·capture/replay/network identity를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=native_policy_admission·native_cohort_policy_admission fixture와 actual launcher의 selected 정책/타이밍 preset envelope를 current source로 대조.
  - [ ] **BK-WBS-07.13.02** Windows/macOS solo/local parser와 launcher가 검증된 정책으로 network owner를 준비한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=Windows/macOS parser·foreign Rust typing과 실제 local consumer에서 validate_selected/header 및 network preparation 호출 순서를 확인; port만 존재하는 상태와 구별.
  - [ ] **BK-WBS-07.13.03** 지원 OS 실제 solo/local network 시작의 정책 일치/불일치를 peer 간 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-07.13.01, BK-WBS-07.13.02; 필요 근거/다음=실제 native inputs/output·QUIC/WebTransport peer에서 correct/mismatched class/gauge rejection과 current capture/handshake를 확인; typed compile은 peer start 아님.
- [ ] **BK-WBS-07.14** 여러 buffer/backend/rate에서 시작·정지·재개·교체의 음향 alignment를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=물리 loopback; 소프트웨어 clock 통과와 별도.
  - [ ] **BK-WBS-07.14.01** 여러 실제 buffer/rate에서 startup·pause/resume·finite end의 음향 onset/offset alignment를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=물리 loopback과 clock-capable endpoint에서 요청/applied buffer/rate·native coordinate·input trace와 실제 sound edge를 동시에 기록.
  - [ ] **BK-WBS-07.14.02** 실제 same/cross-backend 교체의 음향 continuity·latency·실패 복구를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=WASAPI/ASIO/CoreAudio/ALSA 지원 조합과 ASIO SDK/driver 환경에서 교체 전후 PCM/음향·cleanup을 비교; software native clock 통과와 별도.

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
  - [ ] **BK-WBS-08.08.01** 기본 timing dialect와 기존 기본값 변경 여부를 명시적으로 선택한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=사용자가 default dialect·existing default 유지/전환을 결정; opt-in versioned numerical preset 완료를 기본값 승인으로 간주하지 않음.
  - [ ] **BK-WBS-08.08.02** 역사적 LR2/nanasi 호환에서 약속할 key/scratch/LN-end·버전별 의미 범위를 확정한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=원본 engine 문서/primary source 및 재현 asset에 근거한 호환 범위 결정; spec만으로 millisecond 표를 추정하거나 서로 다른 engine을 합치지 않음.
- [ ] **BK-WBS-08.09** 동적 EXRANK/A0 의미와 필요한 timed policy를 primary source로 확인한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=서로 다른 engine 동작을 동일 표준으로 합치지 않음.
  - [ ] **BK-WBS-08.09.01** 동적 EXRANK의 원본 engine별 시간 적용·기본 RANK/DEFEXRANK precedence를 primary source로 감사한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=현행 supported engine primary source를 식별하고 dynamic EXRANK marker 전후 실제 window/gauge/completion 의미를 추적; 기존 cold metadata preset과 구별.
  - [ ] **BK-WBS-08.09.02** A0 timed judge policy의 버전/채널 의미와 replay/network identity 요구를 primary source로 감사한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=원본 engine/source와 chart 사례에서 A0의 실제 적용 시점·범위·중복/STOP 경계를 추적해 독립 timed policy 계약 필요 여부 결정; 다른 engine을 동일 표준으로 합치지 않음.
- [x] **BK-WBS-08.10** fresh unmatched/repeat/refused 입력을 구별하는 authoritative empty-POOR disposition을 만든다. — 상태=D(검증완료); 선행=-; 근거=E32,REQ__input-disposition; penalty/dialect 선택은08.11에 유지.
- [ ] **BK-WBS-08.11** empty-POOR의 후보/반복/게이지/combo/capture 의미를 선택 dialect에 고정한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=preroll 정책과 구별; replay/network identity에 포함.
  - [ ] **BK-WBS-08.11.01** 선택 dialect의 empty-POOR 후보·fresh unmatched/repeat/refused·반복 억제 규칙을 확정한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=REQ__input-disposition의 authoritative disposition D 범위를 유지하고 empty penalty 후보/시각/동시입력 의미를 사용자 정책으로 결정; pre-play 정책과 구별.
  - [ ] **BK-WBS-08.11.02** 선택 empty-POOR의 gauge/combo/EX·capture/replay/network identity 의미를 고정한다. — 상태=P(정책선택대기); 선행=BK-WBS-08.11.01; 필요 근거/다음=확정된 후보 규칙을 실제 gauge/combo와 versioned capture/replay/room identity에 기록; 변경 전후 재판정/원본 disposition 보존을 문서화.
- [x] **BK-WBS-08.12** WAV/FLAC/Vorbis/MP3가 실제 bounded preparation과 Mixer 출력까지 연결된다. — 상태=D(검증완료); 선행=-; 근거=E1; 포맷별 declared trim/limits.
- [x] **BK-WBS-08.13** same-format Ogg chaining의 BOS/EOS/serial/독립 trim/aggregate limit를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E1; malformed·mixed format 전체 거절.
- [x] **BK-WBS-08.14** 전 포맷의 잘린 파일·큰 metadata·late decode 실패·rate/channel 정책을 확장 검증한다. — 상태=D(검증완료); 선행=-; 근거=`255c400`의 독립 DEEP code review 후 CLI QA47/0/0ignored(새 corpus15·allocation5·기존 preparation27), Rust2021 format/diff/WBS 검사 exit0, [준비 corpus/측정 지침](../verification/GUIDE__audio-preparation-corpus.md)과 [검증 기록](../changes/2026-10-10-audio-preparation-corpus.md). 범위=네 포맷의 public decoder·memory/file preparation·후반 codec 오류·metadata·rate/channel·PCM/aggregate/alias·실제 sparse encoded-limit 및 명시된 TLS allocation-request corpus. 총 RAM sandbox/전체 codec conformance·실제 음향·Harness close 증거 아님.
- [ ] **BK-WBS-08.15** static BGA BMP/PNG/JPEG·layer/poor/opacity/crop/canvas·파일명 호환을 실제 화면에서 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=기존 순수 fixture + 실제 native/browser rendering.
  - [ ] **BK-WBS-08.15.01** static BMP/PNG/JPEG 및 확장자/파일명 alias가 bounded preparation·native/browser publication에서 같은 원본 image identity를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=BMS image preparation·bga.rs/bga_render.rs의 decode/path/canonical alias와 role-specific black key를 실제 fixture로 대조; file-name compatibility asset corpus 확인.
  - [ ] **BK-WBS-08.15.02** Base/Layer/Layer2/Poor·opacity·crop·CANVASSIZE가 원본 song timeline과 actual Scene/cache 구성에 일치한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/src/bga_render.rs의 cropped_aliases·Scene order/opacity/cache fixtures와 actual Runtime STOP/replay/native publication을 current source로 재실행.
  - [ ] **BK-WBS-08.15.03** 실제 browser GPU 화면에서 layer/poor/alpha·crop/canvas와 BMP/PNG/JPEG 파일명 호환을 확인한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-08.15.01, BK-WBS-08.15.02; 필요 근거/다음=current WASM 실제 browser runner에서 asset chart·poor trigger·STOP/replay를 실행해 blend pixels/placement와 canonical filename alias를 확인; 순수 Scene fixture는 화면 증거 아님.
  - [ ] **BK-WBS-08.15.04** 실제 native GPU 화면에서 static BGA의 layer/poor/opacity·crop/canvas와 파일명 호환을 확인한다. — 상태=E(환경·장비대기); 선행=BK-WBS-08.15.01, BK-WBS-08.15.02; 필요 근거/다음=current desktop binary·native graphics/입력 장치에서 actual chart와 poor trigger·STOP/replay pixels/placement를 확인; browser software GPU 결과를 native 장치 화면으로 확대하지 않음.
- [ ] **BK-WBS-08.16** EXBMP RGB/color-key/근사 matching의 정확한 호환 규칙을 확정한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=원본 engine 문서/asset 사례·허용 오차.
  - [ ] **BK-WBS-08.16.01** EXBMP RGB/ARGB·color-key와 layer/base/poor의 정확한 호환 규칙을 primary source/asset 사례로 확정한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=원본 engine 문서·실제 EXBMP asset chart를 대조해 channel roles/색 처리/alpha 의미를 선택; 현재 exact-black static layer 규칙을 EXBMP 지원으로 확대하지 않음.
  - [ ] **BK-WBS-08.16.02** EXBMP 근사 matching의 색 공간·오차·경계와 정확히 거절할 범위를 확정한다. — 상태=P(정책선택대기); 선행=BK-WBS-08.16.01; 필요 근거/다음=근사 비교 허용 여부와 tolerance/rounding·unknown format 거절 정책 결정; 임의 threshold나 per-frame pixel scan을 추가하지 않음.
- [x] **BK-WBS-08.17** video decode·timestamp/reorder/frame budget·off-thread 공급을 구현한다. — 상태=D(검증완료); 선행=-; 근거=E34,REQ__bms-video의 순수 계약과 실제 native/browser decode-to-Scene·GPU·플레이 경로; 지원 코덱/배포 선택은08.18에 유지.
- [ ] **BK-WBS-08.18** video codec/backend/redistribution 라이선스와 지원 범위를 선택한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=MIT/non-ASIO 배포 조건과 실제 third-party 감사.
  - [ ] **BK-WBS-08.18.01** native/browser video codec/backend와 플랫폼별 지원/미지원 범위를 선택한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=REQ__bms-video의 bounded decode/off-thread/Scene D 범위를 유지하고 실제 codec/backend availability·fallback 없는 capability 표를 확정.
  - [ ] **BK-WBS-08.18.02** video codec/backend third-party redistribution 라이선스와 MIT/non-ASIO 배포 조합을 확정한다. — 상태=P(정책선택대기); 선행=BK-WBS-08.18.01; 필요 근거/다음=실제 연결 decoder/배포 artifact의 third-party license·notice/source 제공 의무를 audit하고 허용 배포 조합을 선택; 코덱 구현D를 배포 승인으로 간주하지 않음.

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
  - [x] **BK-WBS-09.10.01** Desktop F5 pinned retry와 bookmark 요청의 실제 worker ACK·cleanup·attempt lineage를 software 경로에서 보존한다. — 상태=D(검증완료); 선행=-; 근거=E35; app/src/desktop_practice_control_fixtures.rs의 desktop_practice_control_f5_uses_pinned_start_and_applied_ack_only, desktop_practice_control_spawned_worker_keeps_tls_and_joined_f5_lineage_after_explicit_cancel가 포함된 독립 CLI QA. 주입 native 관측 범위이며 실제 OS 키 입력/장치 증거가 아님; 다음=cargo test -p beatkernel-bms-runtime --bins --features desktop,webtransport --locked desktop_practice_control
  - [ ] **BK-WBS-09.10.02** 실제 Practice 화면에서 북마크·시작/끝 구간 편집·거절 후 수정이 retained owner에 같은 값을 적용한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/src/desktop_practice_control_fixtures.rs와 app/src/practice_control_fixtures.rs; cargo test -p beatkernel-bms-runtime --lib --features desktop,webtransport --locked practice_control 후 실제 GUI에서 editor/F8/F11·invalid section·Back/reentry를 원래 요청/ACK와 대조
  - [ ] **BK-WBS-09.10.03** 실제 native 재생 중 F5·finite loop·disable·scrub가 이전 owner 회수와 원래 section/기록 의미를 유지한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E35 software pump 검사를 실제 OS GUI/출력 관측으로 확장; cargo test -p beatkernel-bms-runtime --lib --features desktop,webtransport --locked gapless_practice 후 실제 finite loop/Stop/F5를 재생·cleanup·attempt archive와 대조
- [x] **BK-WBS-09.11** gapless loop/scrub에서 같은 출력 timeline을 유지하며 reopen gap을 제거한다. — 상태=D(검증완료); 선행=-; 근거=E35; 범위=실제 production owner/pump와 주입 native 관측을 연결한 software E2E; 물리 측정은09.12.
- [ ] **BK-WBS-09.12** 장시간/반복 구간 재시작의 실제 키음/BGM/노트 동기화를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=물리 출력/loopback; 논리 hash만으로 완료하지 않음.
  - [ ] **BK-WBS-09.12.01** 실제 출력의 단일·반복 section 재시작에서 키음/BGM onset과 note target 오차·불확실성을 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=doc/kernel/REQ__section-restart.md와 REQ__gapless-practice.md; 사용 가능한 native 장치/loopback을 확보한 뒤 original PCM frame·requested/applied song ns·실제 output timestamp·음향 onset을 함께 수집
  - [ ] **BK-WBS-09.12.02** 장시간 반복 loop/scrub에서 재시작 횟수별 키음/BGM/노트 drift와 출력 연속성을 측정한다. — 상태=E(환경·장비대기); 선행=BK-WBS-09.12.01; 필요 근거/다음=물리 장치/loopback에서 동일 원본 자산의 반복 cut·actual presentation pair·PCM/음향 누락과 누적 오차 분포 기록; 논리 hash·software 20h/week 순회를 물리 soak로 대체하지 않음
- [ ] **BK-WBS-09.13** profile/replay/archive 저장의 interruption·동시 publish·symlink·capacity 오류를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 부분 근거=E37 개별 replay/archive 및 E38 profile의 실제 파일 I/O·오류 주입·동시 게시·공개 save/load; 필요 근거/다음=프로세스 interruption 후 재실행/recovery·실제 capacity 및 전체 원래 저장 안정성 기준.
  - [x] **BK-WBS-09.13.01** 개별 replay/archive의 staging·exclusive publish·동시 게시·leaf symlink/예약 namespace·I/O 오류를 software 파일 경로에서 검증한다. — 상태=D(검증완료); 선행=-; 근거=E37, doc/changes/2026-10-10-native-record-publication-integrity.md; 실제 public replay17·archive consumer13·shared publisher25의 독립 실행. 파일별 게시 범위이고 group transaction·hostile ancestor·crash recovery는 별도; 다음=cargo test -p beatkernel-bms-runtime --test replay_capture --features desktop,webtransport --locked
  - [x] **BK-WBS-09.13.02** profile v1/v2 public save/load와 교체 재검증·동시 create·stage ownership 회수·symlink 보존을 software 파일 경로에서 검증한다. — 상태=D(검증완료); 선행=-; 근거=E38, doc/runtime/REQ__native-profile-publication.md와 doc/changes/2026-10-10-native-profile-publication-integrity.md; public profile10·실제 adapter race/error fixture·shared publisher28 독립 실행. 외국 OS/native crash는 별도; 다음=cargo test -p beatkernel-bms-runtime --test settings_profile_publication --features desktop,webtransport --locked
  - [ ] **BK-WBS-09.13.03** profile/replay/archive의 프로세스 interruption 지점과 재실행 후 orphan stage·원래 final 보존/recovery를 감사·검증한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=E37/E38는 process interruption/recovery를 제외함; app/src/native_publication.rs와 app/src/native_settings_profile.rs를 읽고 write/flush/sync/link/rename 전후 별도 프로세스 종료·재시작 acceptance를 확정한 후 실행
  - [ ] **BK-WBS-09.13.04** 실제 capacity/disk exhaustion에서 profile/replay/archive final·유효 prefix·원래 오류와 cleanup 결과를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E37/E38의 injected capacity refusal은 실제 disk exhaustion 아님; 별도 소유 제한 용량 filesystem에서 public save/load와 archive group의 ENOSPC·기존 파일 보존·재시도/정리 확인
  - [ ] **BK-WBS-09.13.05** 안정된 부모 디렉터리 가정 밖의 symlink/ancestor 교체 경계와 지원 filesystem의 저장 보장을 명시·검증한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=doc/runtime/REQ__native-profile-publication.md의 hostile ancestor/interprocess locking 한계와 native publication 계약 대조; 실제 ancestor 교체·unsupported hard-link 사례를 감사하고 원래 저장 안정성 기준의 잔여 구현/검사를 기록

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
  - [ ] **BK-WBS-10.15.01** dense tap/hold/mine의 ordered membership·primitive 수와 source 합산 한도를 누락 없이 처리한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=doc/ui/REQ__dense-playfield-presentation.md, app/src/dense_playfield_fixtures.rs의 11520 primitive·64 roster/four-field oracle; cargo test -p beatkernel-bms-runtime --lib --features desktop,webtransport --locked dense_playfield. 현재 source 존재는 실행 PASS 아님
  - [ ] **BK-WBS-10.15.02** GPU 버퍼 성장·실제 device byte/draw 한도와 모든 playfield 사전 검증·실패 원자성을 보존한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=REQ__dense-playfield-presentation.md의 max_buffer_size·source-derived byte cap·late-field poison/clear 규칙과 app/src/graphics.rs/scene 경로; cargo test -p beatkernel-bms-runtime --lib --features desktop,webtransport --locked dense_playfield 후 device-limit 경계/재업로드 검사
  - [ ] **BK-WBS-10.15.03** 현재 native/browser GPU에 8192개 초과 인스턴스를 제출하고 warm/seek/resize·late refusal을 확인한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-10.15.01, BK-WBS-10.15.02; 필요 근거/다음=app/tests/dense_native_gpu.rs와 app/web/dense-playfield.browser.mjs; BEATKERNEL_TEST_NATIVE_UI_WINDOW=1 cargo test -p beatkernel-bms-runtime --features desktop --test dense_native_gpu --locked -- --ignored --test-threads=1 및 node app/web/dense-playfield.browser.mjs. 실제 실행/independent QA 전 D 불가
  - [ ] **BK-WBS-10.15.04** 큰 chart·장시간 projection에서 retained cache 재사용·peak 보관 공간·누락과 비용을 측정한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=REQ__dense-playfield-presentation.md의 old Arc/scratch/GPU replacement peak와 local epoch 규칙; dense_playfield_cache_warms_rebases_seeks_resizes_and_preserves_old_packet_on_refusal 및 실제 장시간 projection trace/primitive oracle를 실행. E43 CPU judge benchmark는 GPU 표시 증거 아님
- [ ] **BK-WBS-10.16** 개별 component fractional transform/opacity/easing을 paint/hit/clip에 공통 적용한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E17/E18 browser Selection/Display, E24 native Selection/Display 소유자, E25 실제 native software Vulkan 이동/역 hit; E40 [Practice 연결](../changes/2026-10-10-practice-component-motion.md)의 공통 API·browser/native 소유자·accepted-pose IME 구현, 독립 code/CLI/browser QA 및 실제 native window/direct PNG 확인. Desktop MCP 동일 창 연결은 BLOCKED_ENV이며 나머지 화면 연동·OS 입력/IME·하드웨어 검증은 필요.
  - [x] **BK-WBS-10.16.01** 공통 retained component의 fractional translation/scale/opacity/easing·inverse hit·clip·slot epoch를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E17/E24/E40 및 doc/changes/2026-10-10-remaining-component-motion.md의 independent public-view25·shared motion/owner54 실행. Source/parent clip·atomic restore·geometry identity 범위; 다음=cargo test -p beatkernel-bms-runtime --lib --features desktop,webtransport --locked component_motion
  - [x] **BK-WBS-10.16.02** Selection/Display/Practice의 실제 browser 및 native software-window motion·accepted pose·geometry 재사용을 확인한다. — 상태=D(검증완료); 선행=-; 근거=E18 browser Selection/Display·E25 native22 frames·E40 Practice 실제 browser39 frames/native window PNG. 실제 software window·direct PNG 범위이고 OS-driven animation/physical IME/Practice desktop MCP blocker 전체 해소를 뜻하지 않음; 다음=doc/changes/2026-10-10-practice-component-motion.md의 실제 재현 경로
  - [x] **BK-WBS-10.16.03** Settings/Records/Players/Devices와 readonly Results의 software owner·실제 browser/native window motion을 연결한다. — 상태=D(검증완료); 선행=-; 근거=doc/changes/2026-10-10-remaining-component-motion.md; 독립 CLI/public-view·browser14 inspected captures·native64 frames/11 PNG·same-window MCP PASS. Genuine solo Results의 comparison/multi-page live 연결은 제외; 다음=node app/web/tests/remaining-motion-browser.mjs
  - [ ] **BK-WBS-10.16.04** comparison·여러 페이지를 가진 실제 Results owner에서 motion 재바인딩·없는 card 퇴역·accepted hits를 확인한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/tests/results_component_motion.rs와 app/src/native_remaining_motion_fixtures.rs의 native_results_partial_page_prunes_absent_cards_and_preserves_surviving_pose는 CLI 범위; 실제 해당 capability를 가진 native/browser Results로 paging·retirement·clip을 관찰
  - [ ] **BK-WBS-10.16.05** OS가 전달한 실제 animated pointer/keyboard/IME 입력과 font 후보 위치가 마지막 제출 pose를 따른다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=REQ__ui-motion.md의 현재 미검증 OS-driven animated hit/physical IME 경계; 실제 native event loop와 물리 IME 환경에서 이동 전후 hit·opacity·clip·candidate anchor를 화면/원래 입력과 대조
- [ ] **BK-WBS-10.17** component animation scheduler의 frame budget·취소·suspend/resume/dispose를 구현한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E17 64-slot/수명/cache·worker scheduling, E18 browser 51프레임/dispose, E24 native 수명 fixture, E25 native 22프레임/cache/dispose; 실제 성능/frame budget 및 독립 QA 필요.
  - [x] **BK-WBS-10.17.01** 64-slot scheduler의 caller time·cancel·suspend/resume·dispose·stale identity 거절을 software 경로에서 검증한다. — 상태=D(검증완료); 선행=-; 근거=E17/E24/E40, doc/changes/2026-10-10-remaining-component-motion.md의 native/browser owner lifecycle·Results retirement·zero extent 실행; 다음=cargo test -p beatkernel-bms-runtime --lib --features desktop,webtransport --locked motion
  - [x] **BK-WBS-10.17.02** 실제 browser/native owner가 animation frame과 surface retry를 분리하고 변경 없는 geometry/cache를 재사용한다. — 상태=D(검증완료); 선행=-; 근거=E17/E18/E25/E40 및 remaining-component-motion의 실제 software browser/native frames·geometry identity/revision 관찰. 하드웨어 frame budget이 아님; 다음=node --test app/web/renderer-worker.test.mjs app/web/tests/remaining-results-motion.test.mjs
  - [ ] **BK-WBS-10.17.03** 동시 animation·메뉴 편집·취소의 frame budget과 bounded overload 동작을 software trace로 측정한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=REQ__ui-motion.md의 64-component bound를 실제 동시 track/편집 workload에 적용하여 scheduling duration·missed frame·queue/backpressure·취소 지연 보고; slot 수나 frame 개수만으로 budget PASS 불가
  - [ ] **BK-WBS-10.17.04** 대상 하드웨어에서 animation frame budget·GPU 제출 비용과 입력 지연을 검증한다. — 상태=E(환경·장비대기); 선행=BK-WBS-10.17.03; 필요 근거/다음=실제 GPU/디스플레이·입력 장비에서 동일 workload trace와 frame/input 분포 기록; SwiftShader/llvmpipe PASS는 hardware latency 증거가 아님
- [ ] **BK-WBS-10.18** font fallback/Unicode/IME/clipboard/selection이 실제 native/browser 입력 방법에서 동작한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=순수 editor fixture + 실제 API/글꼴.
  - [ ] **BK-WBS-10.18.01** 실제 native/browser font fallback과 Unicode glyph·편집 selection의 표시를 확인한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/src/desktop_ime_fields_fixtures.rs 및 font 준비 경로; cargo test -p beatkernel-bms-runtime --bins --features desktop,webtransport --locked ime 후 실제 fallback font/한글·결합문자·미지원 glyph·선택 강조 화면 대조
  - [ ] **BK-WBS-10.18.02** 실제 OS IME preedit/commit·focus 전환과 transformed candidate 위치를 native/browser에서 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/src/desktop_ime_area_fixtures.rs와 E27/E40 bridge evidence; 실제 OS IME에서 field 변경·Back/resize·accepted pose·취소를 실행. 주입 composition과 Rust candidate fixture는 물리 IME 완료가 아님
  - [ ] **BK-WBS-10.18.03** 실제 clipboard read/write와 selection·permission 취소가 원래 editor owner와 제한을 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/src/desktop_clipboard_fixtures.rs와 app/web/main.js의 실제 clipboard bridge; native/browser 키보드 복사·붙여넣기·거절/권한취소·stale field를 실제 API로 확인
- [ ] **BK-WBS-10.19** 0-size/resize/DPI/focus loss/renderer loss/close가 owner cleanup과 input geometry를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 winit/GPU/browser lifecycle.
  - [ ] **BK-WBS-10.19.01** 0-size·resize·DPI 변화에서 제출된 geometry와 pointer/IME 위치·suspend/resume를 일치시킨다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E40 및 app/src/desktop_ime_area_fixtures.rs의 부분 근거; 실제 winit/Browser viewport를 zero/odd size/여러 DPR로 바꾸고 마지막 성공 draw extent·hit·candidate anchor 확인
  - [ ] **BK-WBS-10.19.02** focus loss와 close가 held input·composition·owner stop/join을 원래 순서로 정리한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=desktop.rs 및 app/web/main.js focus/visibility/close 경로; 실제 키/접촉 hold 중 blur·window close·navigation에서 prefix·release·joined cleanup·late callback 거절 기록
  - [ ] **BK-WBS-10.19.03** renderer/device/surface loss와 재시도가 부분 제출 없이 입력 geometry·cache·owner cleanup을 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/web/renderer-worker.test.mjs의 unpresented/retry/retirement fixture 및 native graphics recovery; 실제 GPU/device/surface loss를 유발해 recovery 또는 명시 stop과 자원 회수를 확인
- [ ] **BK-WBS-10.20** Selection→설정→로딩→플레이→pause/output→기록/결과→Back/재시작 전 흐름을 GUI로 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 shader/클릭/키/수명·독립 GUI QA.
  - [ ] **BK-WBS-10.20.01** Selection→Settings/Practice/Display→Loading·Cancel·오류수정 경로를 실제 GUI로 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E39/E40/E42의 부분 journey와 doc/changes/2026-10-11-browser-retained-settings-policy.md, browser-display-semantics.md의 최종 strict AC-005 FAIL을 함께 유지; node app/web/settings-policy.browser.mjs 후 actual native startup/menus도 관찰. 과거 일부 PASS로 whole flow 승격 불가 현재=doc/changes/2026-10-15-browser-audio-current-acceptance.md의 두 fresh13journey는 끝까지 실행됐으나 Chromium153의 strict network FAIL로 acceptance 미완료; 최종 cleanup error0·V 유지.
  - [ ] **BK-WBS-10.20.02** 실제 Play→pause→Output 변경→resume/F5 재시작이 GUI 명령·native owner ACK와 일치한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E55 actual Settings/Start 이후 evdev/audio 장치 부재 BLOCKED_ENV; 실제 장치/입력이 있는 GUI에서 acknowledged pause·Output·resume·capture·F5를 연속 검증. software owner helper는 실제 Play 증거 아님
  - [ ] **BK-WBS-10.20.03** 자연 완료→기록 저장/선택→Results→Back/다음 실행이 genuine 결과·prefix와 수명을 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E39/E42와 실제 current browser/native 전체 journey; node app/web/render-integration.browser.mjs의 genuine-completed/local-member-history와 ordinary GUI navigation을 원래 replay/archive/ACK·screenshots·cleanup으로 대조. loading ERR_ABORTED와 불완전 native flow는 별도 미완료

### 11. 브라우저 WASM·Worker·오디오·입력

기능 연결: BK-040, BK-041, BK-042, BK-045, BK-047, BK-050, BK-051, BK-052, BK-053. 요구: REQ__bms-browser / browser-retained-menus.

- [x] **BK-WBS-11.01** 현재 source의 실제 WASM library/AudioWorklet/render package를 빌드한다. — 상태=D(검증완료); 선행=-; 근거=E39; pinned wasm-bindgen 0.2.129·native-free dependency 검사 및 실제 current package 실행 확인. 전체 browser/runtime acceptance는 별도.
- [ ] **BK-WBS-11.02** Gameplay Worker가 준비·판정·capture·network owner를 담당한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=현재 실제 bridge·stalled renderer 관찰.
  - [x] **BK-WBS-11.02.01** 실제 gameplay Worker의 solo/local 준비·판정·capture가 현재 WASM과 원래 replay 결과를 사용한다. — 상태=D(검증완료); 선행=-; 근거=E39/E41의 actual6초 capture→history→replay·keyboard/touch local 및 원래 점수1/4/0/1 independent browser 실행. 이 bounded Worker gameplay 범위는 network/renderer-stall acceptance 아님; 다음=node --experimental-vm-modules --test app/web/worker.test.mjs app/web/play-worker.test.mjs
  - [ ] **BK-WBS-11.02.02** 실제 browser network/room owner가 gameplay Worker에 남고 local authority와 분리된다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/web/worker.js, room-owner.mjs, app/src/browser_room_client.rs 및 doc/runtime/REQ__browser-room-play.md; node --experimental-vm-modules --test app/web/room-owner.test.mjs app/web/play-worker.test.mjs 후 strict 실제 room 관찰. 주입 transport PASS는 actual HTTP/3 ownership 완료 아님
- [ ] **BK-WBS-11.03** Renderer Worker/OffscreenCanvas가 GPU와 local retained views를 소유한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 current WASM renderer·Scene/메뉴 수명.
  - [x] **BK-WBS-11.03.01** Renderer Worker/OffscreenCanvas의 검증된 BGA·Records·motion 경로가 해당 Scene/GPU와 readonly retained view를 소유한다. — 상태=D(검증완료); 선행=-; 근거=E34 실제 browser decode-to-Scene/GPU·E39 actual package/GPU 화면·E42 retained Records browser, E40/remaining-motion actual Worker software render. 범위=해당 실제 시연된 visual owners; 다음=node --test app/web/renderer-worker.test.mjs
  - [ ] **BK-WBS-11.03.02** Renderer view 교체·suspend/dispose·surface 재시도 시 실제 OffscreenCanvas/GPU 자원과 local owner를 회수한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/web/renderer-worker.test.mjs의 delayed initialization/replacement/retirement, app/src/browser_render_state_fixtures.rs; node app/web/render-integration.browser.mjs의 ready-preview/mode-admission-atomic 및 실제 renderer loss/close 검사
- [ ] **BK-WBS-11.04** 모든 메뉴의 business owner·상태 packet·semantic action을 실제 Renderer view에 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=Selection/Settings/Practice/Display/Records/Players/Devices/Results.
  - [ ] **BK-WBS-11.04.01** Selection/Settings/Practice/Display의 실제 business draft·editor·지원 action을 retained renderer에 왕복 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/src/browser_menu_fixtures.rs·E40/E41와 settings-policy/browser-display-semantics의 후속 strict FAIL; node app/web/settings-policy.browser.mjs로 Apply/Back/reopen/profile/Practice를 fresh current package에서 확인 현재=doc/changes/2026-10-15-browser-audio-current-acceptance.md의 두 fresh13journey는 끝까지 실행됐으나 Chromium153의 strict network FAIL로 acceptance 미완료; 최종 cleanup error0·V 유지.
  - [ ] **BK-WBS-11.04.02** Players/Devices가 canonical roster·획득 inventory·페이지/할당 gate를 실제 retained menu action과 동기화한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E27/E29 및 app/src/browser_menu_fixtures.rs의 21→10/20→5→empty regression; 실제 Discover/assign/retire/rediscover·Players Done·Devices paging을 genuine inventory로 확인. motion-only browser PASS는 acquisition 완료 아님
  - [ ] **BK-WBS-11.04.03** Records·Results의 supported action·readonly history/score page와 route 수명을 실제 renderer에 연결한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-11.05; 필요 근거/다음=BK-WBS-11.05/E42의 Records 완료를 재사용하고 Results genuine/comparison/room page capability 잔여 검사; node app/web/render-integration.browser.mjs의 genuine-completed/local-member-history/combined-results-room. whole menu/full Results 완료는 별도
- [x] **BK-WBS-11.05** 실제 Records preview·own/other opponent·history를 read-only packet으로 연결한다. — 상태=D(검증완료); 선행=-; 근거=E42; AC024 모델과 actual retained browser navigation의 독립 감사 완료.
- [ ] **BK-WBS-11.06** bounded packet·generation/screen/revision 토큰이 stale/hostile action을 원자적으로 거절한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 generated WASM + Worker/RenderClient.
  - [ ] **BK-WBS-11.06.01** bounded packet의 count/byte/UTF-8·full-u64·kind/extent 검증이 실제 generated WASM 전에 거절한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/web/render-protocol.test.mjs, browser-menu.test.mjs 및 app/src/browser_render_state_fixtures.rs; node --test app/web/render-protocol.test.mjs app/web/browser-menu.test.mjs 후 current generated WASM boundary corpus를 실행
  - [ ] **BK-WBS-11.06.02** generation/content/screen/revision·submitted geometry가 stale/hostile action을 부분 변경 없이 거절한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/web/render-integration.browser.mjs의 mode-admission-atomic/live-stalled-input-audio·app/web/renderer-worker.test.mjs; RENDER_INTEGRATION_CASE=mode-admission-atomic node app/web/render-integration.browser.mjs로 실제 GPU image/원래 sequence 보존 확인
- [ ] **BK-WBS-11.07** Window에 렌더/준비/대규모 model 갱신을 되돌리지 않고 입력·필수 gesture bridge만 둔다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=main thread CPU trace 및 source audit.
  - [ ] **BK-WBS-11.07.01** Window에 남은 작업을 physical input·필수 trusted gesture/IME/file bridge와 event-driven 상태로 감사한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=doc/ui/ADR__browser-worker-boundary.md·doc/kernel/REQ__bms-browser.md와 app/web/main.js/worker.js/renderer-worker.js의 호출·소유권 대조; 준비/대규모 model/판정/게임 rendering loop가 Window로 돌아오는 경로 감사
  - [ ] **BK-WBS-11.07.02** 실제 import/preparation/play/Results에서 Window CPU trace와 입력 수집 비용을 측정한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=Chromium main-thread CPU trace를 파일 import·dense gameplay·local page·room HUD와 renderer stall 동안 수집; source ownership만으로 main-thread budget/latency PASS를 주장하지 않음
- [ ] **BK-WBS-11.08** AudioWorklet이 실제 callback·output clock·buffer·finite drain을 처리한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 browser audio; mock clock만으로 완료하지 않음.
  - [x] **BK-WBS-11.08.01** Chromium의 실제 native callback frame 인증·원래 output clock·arm/refill을 44.1/48kHz에서 검증한다. — 상태=D(검증완료); 선행=-; 근거=doc/changes/2026-10-14-browser-native-clock-product.md의 final independent DEEP/CLI/browser PASS; actual cold/rolling/live/replay/marker/single/joint77 required checks와 실제 WASM hashes. Chromium153/native128-frame software 범위이며 task receipt PENDING·물리 음향/다른 browser/aged context는 별도; 다음=WORKLET_NATIVE_CLOCK_PRODUCT=1 node app/web/worklet-chronology.browser.mjs
  - [ ] **BK-WBS-11.08.02** 실제 AudioWorklet buffer pressure·gap/overflow·finite end/drain이 solo/local/replay 종료·오류에서 정확하다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E39와 native-clock-product의 제한된 live/replay PASS 및 과거 status6/ERR_ABORTED 원본을 보존; node --experimental-vm-modules --test app/web/audio-worklet.test.mjs app/web/audio-host.test.mjs app/web/finite-output-report.test.mjs 후 actual finite/partial buffer·local/replay·suspend/stop/replacement failure를 원래 callback/report/cleanup으로 대조
  - [ ] **BK-WBS-11.08.03** 다른 실제 context rate/extent·장치 변경·장시간 native context에서 output clock/finite drain 지원과 refusal을 확인한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=REQ__browser-native-context-clock.md의 Chromium128-frame/44.1·48kHz ceiling; 실제 적용된 새 rate/extent·device change/aged context를 관찰하고 capability/refusal·clock uncertainty를 기록. pure one-frame/week vectors를 native 실행으로 대체하지 않음
- [x] **BK-WBS-11.09** portable Worklet 입력/출력 report의 extent·gap·overflow·queue/start/end 산술을 검증한다. — 상태=D(검증완료); 선행=-; 근거=E1의 worklet_audio_fixtures.
- [ ] **BK-WBS-11.10** keyboard/touch/pointer/HID/gamepad가 원래 source·timestamp·binding을 유지한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 browser 입력과 지원 capability.
  - [ ] **BK-WBS-11.10.01** 실제 keyboard/touch/pointer acquisition이 original source·timestamp·contact/order를 Worker binding/capture까지 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E39/E41 ordinary keyboard/touch 부분 근거, app/web/physical-input.test.mjs·pointer-input.test.mjs·app/src/browser_input_fixtures.rs; 실제 trusted/coalesced 입력의 raw timestamp·source·acquired-prefix와 capture를 대조
  - [ ] **BK-WBS-11.10.02** 실제 HID report와 Gamepad attachment/sample이 full source identity·원래 timestamp·profile binding을 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/web/hid-input.test.mjs·gamepad-input.test.mjs·app/src/browser_hid_runtime_fixtures.rs; node --test app/web/hid-input.test.mjs app/web/gamepad-input.test.mjs 후 실제 장치 authorize/disconnect/reconnect·sample/capture 확인
  - [ ] **BK-WBS-11.10.03** resize·page 전환·lost capture·blur/cancel에도 held contact/lane과 입력 sequence가 일관된다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=doc/ui/REQ__browser-input-isolation.md와 app/src/browser_touch_fixtures.rs·browser_local_input_fixtures.rs; RENDER_INTEGRATION_CASE=local-touch node app/web/render-integration.browser.mjs 후 실제 장치 cancel/resize/page 흐름 검증
- [ ] **BK-WBS-11.11** 1인 자동/다인 source 선택·retired ID 미재사용·cancelled chooser를 실제 흐름에서 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=genuine roster import/export와 permission.
  - [ ] **BK-WBS-11.11.01** one-player automatic Any-source와 multi-player distinct-source admission을 실제 ordinary Play 흐름에서 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E39 local keyboard/touch와 app/web/local-play-host.test.mjs·local-play-model.test.mjs; 실제 1인 자동→2인 Discover/assign→Play·잘못된 중복/없는 source 거절을 canonical roster/원래 capture와 대조
  - [ ] **BK-WBS-11.11.02** roster/import-export·member 삭제/재추가가 next-ID·retired ID 미재사용과 literal selection을 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E27/E29·app/src/browser_menu_fixtures.rs actual roster shrink; node --test app/web/local-play-model.test.mjs app/web/host_model.test.mjs 후 실제 roster roundtrip·삭제/재추가·page shrink 관찰
  - [ ] **BK-WBS-11.11.03** cancelled chooser·release/rediscover·retired inventory가 stale assignment를 되살리지 않는다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/web/local-play-host.test.mjs·physical-input.test.mjs·browser-menu.test.mjs; 실제 permission chooser 취소·device disconnect·inventory ACK 순서·retained Devices/Players 재진입 확인
- [ ] **BK-WBS-11.12** IME/file/clipboard/HID/audio permission의 trusted gesture와 취소를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=비동기 Worker action이 권한 클릭을 합성하지 않음.
  - [ ] **BK-WBS-11.12.01** 실제 file/profile/clipboard·IME bridge의 trusted gesture와 비동기 취소·원래 editor correlation을 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E27/E40 및 app/web/host_model.test.mjs·worker.test.mjs의 profile pending/admission; 실제 trusted file/clipboard gesture·취소·IME commit/Back를 current token/field/ACK와 대조. 주입 composition은 OS IME 증거 아님
  - [ ] **BK-WBS-11.12.02** 실제 HID chooser/audio resume가 직접 trusted 사용자 동작에서 열리고 거절·취소를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/web/main.js·audio-host.mjs·physical-input.mjs; 실제 requestDevice/resume와 permission denial/cancel/stop 중 late completion 검사. WORKLET_CHRONOLOGY_REQUIRE_COLD=1 node app/web/worklet-chronology.browser.mjs는 clock cold 부분 재현
  - [ ] **BK-WBS-11.12.03** Worker의 delayed action·stale reply·navigation이 사용자 activation을 합성하거나 다른 owner에 권한 효과를 적용하지 않는다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/web/host_model.test.mjs·play-host.test.mjs의 늦은 gesture/owner regression; 실제 gesture 후 대기·Back/close·새 owner에서 prompt/permission/clipboard/file action을 관찰
- [ ] **BK-WBS-11.13** SAB/COOP/COEP와 message fallback의 지원·실패 경계를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 배포 header·stall/backpressure.
  - [ ] **BK-WBS-11.13.01** 실제 배포의 secure context·COOP/COEP/CORP header와 isolation 누락/실패를 확인한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/web/dense-playfield.browser.mjs와 render-integration.browser.mjs의 crossOriginIsolated 관찰; actual header/CSP·cross-origin asset·미격리 응답 matrix를 serve하여 명시 refusal/지원 범위를 기록
  - [ ] **BK-WBS-11.13.02** SAB 지원 여부·공유 backing 거절과 message 경로의 실제 지원 계약을 감사한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=rg -n SharedArrayBuffer app/web의 대부분은 hostile shared-buffer refusal fixture이고 production SAB transport 구현 증거가 아님; audio/render/input 전송 경로와 원래 SAB/fallback 요구를 대조하여 지원·미지원·잔여 구현을 명시. 현 코드 존재만으로 SAB 완료 불가
  - [ ] **BK-WBS-11.13.03** 실제 message/transfer 경로의 stall·backpressure·detached/late packet이 bounded ownership과 failure를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=node --test app/web/render-protocol.test.mjs app/web/audio-sample-client.test.mjs app/web/audio-command-client.test.mjs 및 actual held-ACK browser 시나리오; fallback 지원 여부 감사 결과에 맞춰 non-SAB 경로를 실행
- [ ] **BK-WBS-11.14** render/GPU stall과 resize가 gameplay/audio/입력 acquired-prefix에 영향을 주지 않는다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=의도적 stalled Renderer·submitted geometry·성능 trace.
  - [ ] **BK-WBS-11.14.01** 실제로 멈춘 Renderer에도 gameplay/audio가 진행하고 원래 acquired input prefix·capture를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=RENDER_INTEGRATION_CASE=live-stalled-input-audio node app/web/render-integration.browser.mjs; 실제 Debugger.pause·trusted keyboard/touch·audio ACK·현재 generated WASM prefix를 확인. runner source만으로 PASS 아님
  - [ ] **BK-WBS-11.14.02** stall/resize/page 사이 입력 hit는 마지막 제출 geometry를 사용하고 누락 frame 뒤 cumulative state를 복구한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/web/render-integration.browser.mjs의 held-ACK/cumulative-after-ack·local-touch, app/web/render-protocol.test.mjs의 submitted tuple; 실제 resize 중 touch/held input·stale geometry 거절을 대조
  - [ ] **BK-WBS-11.14.03** GPU stall·render overload의 main/gameplay/audio trace로 일정·입력·callback 간섭 경계를 측정한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=actual stalled Renderer와 dense/local workload에서 Worker/Main/AudioWorklet trace·input acquired time·buffer/report 분포 수집. 한 번의 기능 진행이나 software frame 수는 성능 분포 증거가 아님
- [ ] **BK-WBS-11.15** Worker 오류·lost renderer·audio failure·navigation close가 joined stop을 수행한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 owner retirement/cleanup.
  - [ ] **BK-WBS-11.15.01** Renderer/GPU 실패가 원래 capture prefix를 먼저 회수하고 renderer owner를 한 번만 stop/dispose한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=RENDER_INTEGRATION_CASE=live-stalled-input-audio node app/web/render-integration.browser.mjs의 malformed-correlated-rejection/terminal-capture-cleanup; 실제 lost renderer/device에서도 prefix·audio close·worker join을 관찰
  - [ ] **BK-WBS-11.15.02** gameplay Worker·AudioWorklet 실패가 original cause·partial recording·실제 stop ACK/cleanup을 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=node --test app/web/audio-failure.test.mjs app/web/audio-host.test.mjs app/web/play-host.test.mjs; native-clock-product의 repeated startup/queued refill fix 부분 근거와 실제 fatal/status/audio context 오류·누락 ACK 관찰을 구분
  - [ ] **BK-WBS-11.15.03** navigation close·replacement 중 pending prepare/network/storage continuation이 이전 owner에만 정리된다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=node --experimental-vm-modules --test app/web/worker.test.mjs app/web/room-owner.test.mjs app/web/play-worker.test.mjs; 실제 탭 close/새 play·held file read/room drain을 걸고 late mutation·false joined success·자원 누수 확인
- [ ] **BK-WBS-11.16** 지원 브라우저와 실제 touch/HID/gamepad에서 API/permission/clock 한계를 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=브라우저·장치 matrix; 미지원은 명시 refusal.
  - [ ] **BK-WBS-11.16.01** 지원 browser 엔진별 secure API·permission·output clock의 실제 가능/미지원 경계를 기록한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=실제 지원 browser/OS runner 확보 후 WebGPU/OffscreenCanvas/AudioWorklet/clock·gesture·HID capability matrix를 실행; Chromium153 software 한 종류 PASS를 전체 지원으로 확대하지 않음
  - [ ] **BK-WBS-11.16.02** 실제 touch/pointer 장치에서 coalesced movement·capture/cancel·timestamp와 viewport 입력을 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=물리 touch/pointer 장치로 app/web/pointer-input.mjs·physical-input.mjs를 실행하고 원래 coalesced samples·predictions exclusion·page/resize prefix 확인. CDP touch는 물리 장치 증거 아님
  - [ ] **BK-WBS-11.16.03** 실제 HID/Gamepad 장치에서 permission·attachment ID·disconnect·sample clock 한계를 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=지원되는 물리 HID/Gamepad와 browser를 확보하여 profile/authorize·재연결·동시 source·보고 시계/권한 취소·explicit unsupported refusal 기록

### 12. QUIC·WebTransport·멀티플레이

기능 연결: BK-038, BK-039, BK-040, BK-041, BK-042, BK-043. 요구: selected-policy-network; multiplayer/room contracts.

- [x] **BK-WBS-12.01** native QUIC의 실제 connect/accept/cleanup이 같은 Tokio runtime 문맥에서 동작한다. — 상태=D(검증완료); 선행=-; 근거=E11 실제 connect/accept·positive exchange·pending cancellation/joins; SIGABRT 회귀 수정.
- [x] **BK-WBS-12.02** 인증서 CA/server-name 오류와 identity 불일치를 실제 QUIC에서 거절한다. — 상태=D(검증완료); 선행=-; 근거=E12 실제 별도 CA/name/identity negative 사례와 no Connected/Ready/start 및 bounded join.
- [x] **BK-WBS-12.03** 실제 WebTransport native client와 HTTP/3 relay room을 상호 연결한다. — 상태=D(검증완료); 선행=-; 근거=E14 실제 HTTP/3 native4-case와 owned relay 종료; browser는 12.04 별도.
- [ ] **BK-WBS-12.04** 브라우저가 actual BrowserRoomOwner/WASM identity로 WebTransport room을 재생한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=[실제 room 플레이 계약](../runtime/REQ__browser-room-play.md)과 strict 2-host acceptance runner 준비. Private NSS CA의 HTTPS 수용/다른 CA 거절은 확인했으나 실제 QUIC certificate handshake가 거절되어 BLOCKED_ENV; eligible system-trusted HTTP/3 endpoint의 두 host 자연 완료·peer score·final ACK/drain 및 native/browser 상호운용 필요. Node47/syntax나 certificate bypass는 완료 근거가 아님.
  - [ ] **BK-WBS-12.04.01** ordinary trust 검증을 통과한 실제 BrowserRoomOwner/WebTransport stream을 확보한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=doc/verification/GUIDE__browser-room-acceptance.md의 private NSS HTTPS 성공·실제 QUIC CERTIFICATE_VERIFY_FAILED/BLOCKED_ENV; system-trusted HTTP/3 endpoint로 실제 stream acquisition 재검증. certificate bypass·direct protocol fixture는 완료 근거 아님
  - [ ] **BK-WBS-12.04.02** 현재 production Main/Workers/WASM의 두 host가 실제 Play·keyboard judging·자연 완료·peer score·final ACK/drain을 수행한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-12.04.01; 필요 근거/다음=doc/runtime/REQ__browser-room-play.md와 node app/web/webtransport-play.browser.mjs; 필요한 trusted app/room URL·독립 focused hosts·current hash 환경을 제공하고 두 original recording/result/ACK/drain·화면·cleanup 확인
  - [ ] **BK-WBS-12.04.03** native와 browser의 실제 room gameplay가 같은 admitted identity·start·final/drain과 종료 의미로 상호운용한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-12.04.01; 필요 근거/다음=현재 native room owner와 production browser owner를 eligible HTTP/3 endpoint에 연결하여 compatible/incompatible policy·실제 local judging·remote prefix·양측 final/drain을 검증; browser-only2 host PASS로 대체하지 않음
- [ ] **BK-WBS-12.05** Origin/trust/path/limits·setup cancellation·pending handshake stop/join을 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 연결/UDP traffic·no false Connected/Ready.
  - [x] **BK-WBS-12.05.01** native room의 actual Origin/CA 거절과 두 peer cancellation/worker join을 검증한다. — 상태=D(검증완료); 선행=-; 근거=E12 unrelated CA/name negatives 및 E14 real_origin_and_untrusted_ca_refuse_before_room_admission·real_room_leave_cancels_cohort_and_fences_committed_owner의 실제 HTTP/3 실행. 해당 두-host native scope만 D; 다음=cargo test -p beatkernel-bms-runtime --features desktop,webtransport --test multiplayer_webtransport_room_loopback --locked -- --ignored --test-threads=1
  - [ ] **BK-WBS-12.05.02** 실제 path·Origin·trust·room/host/byte 제한이 admission 전에 거절하고 false Connected/Ready를 만들지 않는다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/src/multiplayer_webtransport_fixtures.rs의 request_admission_requires_exact_room_and_origin_with_explicit_native_opt_in와 실제 relay negative URL/host limits·UDP 관찰; trusted browser negatives도 strict endpoint에서 확인
  - [ ] **BK-WBS-12.05.03** browser/native setup·pending handshake 취소가 late acquired stream까지 join하고 제한 시간/원래 오류를 유지한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/web/multiplayer-transport.test.mjs의 one setup deadline/cancel·app/web/room-owner.test.mjs cancelled opening과 E11 native pending connect/accept 부분 근거; 실제 handshake 지연/취소·late stream·no false Ready·socket/worker 회수 검증
- [x] **BK-WBS-12.06** 선택 class/gauge/profile/section/seed의 canonical network identity를 순수 검증한다. — 상태=D(검증완료); 선행=-; 근거=E1.
- [x] **BK-WBS-12.07** actual selected cohort owner가 원래 member별 policy header를 port로 전달한다. — 상태=D(검증완료); 선행=-; 근거=E8; 실제 QUIC worker 종료 포함.
- [ ] **BK-WBS-12.08** 선택 정책을 모든 native/browser solo/cohort 실제 네트워크 흐름에 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=기본 wire 의미 유지·capture disabled admission 포함.
  - [ ] **BK-WBS-12.08.01** native solo와 selected/generic/audio-authoritative cohort launcher가 실제 선택 policy·capture-disabled admission을 network owner에 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E8/E28 부분 실행, doc/kernel/REQ__selected-policy-network.md·app/src/native_policy_network_fixtures.rs·native_cohort_network_fixtures.rs; cargo test -p beatkernel-bms-runtime --lib --features desktop,webtransport --locked policy_network 후 실제 launcher→peer 전 범위와 no-capture mismatch 전효과 거절 확인
  - [ ] **BK-WBS-12.08.02** browser solo/local cohort가 현재 pristine game identity로 선택 policy의 실제 peer admission을 수행한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/src/competition_policy_network_fixtures.rs와 app/web/play-worker.test.mjs; current strict room/peer에서 matching policy 및 class/gauge/profile/section/end/seed mismatch를 실제 Play 전에 거절. Node identity fixture는 TLS browser 실행 아님
  - [ ] **BK-WBS-12.08.03** native/browser 상호운용에서 default wire byte 의미·ordered member policy·불일치 prefix 거절을 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=competition_policy_network_fixtures.rs의 default_canonical_network_bytes_equal_original_replay_encoding_for_all_capture_domains 및 E28 selected actual exchanges를 기준으로 actual mixed transport/owner 조합 대조; full native room과 browser interoperability는 잔여 범위
- [ ] **BK-WBS-12.09** room roster·host seal·ready barrier·participant ID와 취소/탈퇴를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 2명 이상 cohort 및 3/4명 이상 room.
  - [ ] **BK-WBS-12.09.01** collecting roster·host seal·ready barrier·full-width participant/member ID를 portable state에서 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/src/multiplayer_group_room_fixtures.rs의 two_three_four_and_sixty_four_hosts_freeze_scoped_rosters_until_every_host_is_prepared와 seal/capacity/deadline fixtures; cargo test -p beatkernel-bms-runtime --lib --features desktop,webtransport --locked multiplayer_group_room
  - [ ] **BK-WBS-12.09.02** 실제 3/4명 이상 room과 2명 이상 local cohort에서 seal/Ready/start 순서와 roster를 대조한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E14 실제2-host·app/src/multiplayer_webtransport_group_fixtures.rs의 in-memory2/3/4 pump는 부분 근거; owned HTTP/3 relay와 3/4 actual clients/여러 local members로 참가자·순서·barrier를 관찰
  - [ ] **BK-WBS-12.09.03** join/Ready/commit 중 취소·탈퇴가 정확한 room lease만 해제하고 replacement participant를 건드리지 않는다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/src/multiplayer_webtransport_group_fixtures.rs의 expected_peer_retirement/stale release·app/tests/multiplayer_webtransport_room_loopback.rs real_room_leave_cancels_cohort_and_fences_committed_owner; 실제 다중 host 단계별 Leave/disconnect/재가입 검증
- [ ] **BK-WBS-12.10** 측정 software start/preroll/committed schedule가 같은 song target에 연결된다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=native/browser 실제 start; 물리 동시 소리는 별도.
  - [x] **BK-WBS-12.10.01** 실제 compatible native peer의 clock/start exchange가 하나의 committed software schedule을 전달한다. — 상태=D(검증완료); 선행=-; 근거=E11 compatible_peers_commit_start_exchange_exact_progress_and_acknowledge_both_finals와 E28 selected actual QUIC/HTTP3 start 교환. Peer protocol software schedule 범위이고 output/음향 동시 시작은 아님; 다음=cargo test -p beatkernel-bms-runtime --features desktop,webtransport --test multiplayer_quic_loopback --locked -- --ignored --test-threads=1
  - [ ] **BK-WBS-12.10.02** native output 준비·preroll·committed target이 실제 original song target과 output coordinate로 이어진다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/src/multiplayer_room_clock_fixtures.rs·multiplayer_room_start_fixtures.rs, E23 source/target startup 부분 근거; 실제 native consumer에서 output observation·late/refused start·first source frame·software target을 함께 관찰
  - [ ] **BK-WBS-12.10.03** browser Worklet 실제 readiness/output grid와 room committed schedule이 같은 song target을 활성화한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=doc/runtime/REQ__browser-room-play.md·REQ__browser-native-context-clock.md; node app/web/webtransport-play.browser.mjs에서 실제 Ready/commit/start·native callback origin·preroll·원래 첫 sample/song time을 대조. Native clock 단독 PASS는 room start 연결 아님
- [ ] **BK-WBS-12.11** progress/results/final ACK/drain·disconnect가 로컬 판정과 원래 prefix를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=full end/finite end/반복 disconnect 실제 exchange.
  - [x] **BK-WBS-12.11.01** 실제 native peer가 ordered progress/finals·별도 final ACK·coordinated drain과 join을 교환한다. — 상태=D(검증완료); 선행=-; 근거=E11/E14/E28 actual compatible/selected exchanges의 ordered progress/finals/drain·worker/relay 종료. Direct protocol exchange 범위이며 실제 gameplay 자연 완료나 모든 disconnect 아님; 다음=cargo test -p beatkernel-bms-runtime --features desktop,webtransport --test multiplayer_webtransport_room_loopback --locked -- --ignored --test-threads=1
  - [ ] **BK-WBS-12.11.02** 실제 full/finite gameplay 종료가 원래 local 판정·record prefix와 remote display·Results·final/drain을 구분한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/src/multiplayer_room_progress_fixtures.rs·app/web/worker-result-archive.test.mjs·room-owner.test.mjs; actual native/browser 자연/full/finite Play에서 local capture hash·genuine final write/ACK/Ready/drain과 frozen Results를 대조
  - [ ] **BK-WBS-12.11.03** 반복 disconnect·Leave·final wait/failed drain이 partial prefix·원래 오류·cleanup receipt를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/src/multiplayer_webtransport_group_fixtures.rs의 pre_drain_disconnect와 app/web/room-owner.test.mjs drain cancellation/deadline; 실제 play 전후·final 전후·교체 중 연결을 끊고 old-owner late callback·local 계속재생·record completeness·joined outcome 확인
- [ ] **BK-WBS-12.12** 연결/peer 입력/크기/속도 제한과 malformed protocol 처리·보안 경계를 audit한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=parser/fuzz + 실제 TLS/Origin/서비스 자원.
  - [ ] **BK-WBS-12.12.01** malformed framing·identity·extent·count·fragment/coalesced protocol 입력을 bounded parser/fuzz corpus로 거절한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/src/multiplayer_protocol_fixtures.rs와 multiplayer_room_wire_fixtures.rs의 malformed header/body/semantic·every fragment boundary; cargo test -p beatkernel-bms-runtime --lib --features desktop,webtransport --locked multiplayer_room_wire 후 fuzz/property corpus와 allocation-before-length 거절 감사
  - [ ] **BK-WBS-12.12.02** 실제 service의 연결·room/peer·byte/속도/queue 제한과 slow/incomplete frame timeout을 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=app/src/multiplayer_webtransport_fixtures.rs의 real_relay...backpressure/cancellation_deadlines 및 multiplayer_webtransport_group_fixtures.rs; 실제 UDP/TLS clients에서 max/overflow·slow fragment·blocked recipient·pending handshake 부하와 회수/자원 상한 관찰
  - [ ] **BK-WBS-12.12.03** TLS/Origin/credential/path·peer data와 local authority 분리의 실제 보안 경계를 audit한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=doc/kernel/REQ__multiplayer-credential-loading.md·REQ__selected-policy-network.md와 current relay/client code를 독립 보안 감사; actual trusted/untrusted/forged roster·malformed peer prefix 사례와 secret/path·local judge/clock 오염 여부 확인
- [ ] **BK-WBS-12.13** 서로 다른 OS/browser의 실제 플레이·latency/clock/start 분포를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=다중 host/device; remote prefix를 로컬 시계로 사용 금지.
  - [ ] **BK-WBS-12.13.01** 서로 다른 실제 OS/native/browser host가 compatible gameplay와 원래 identity·prefix/final을 상호운용한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=독립 actual OS/browser hosts·실제 입출력·trusted endpoint 확보; 각 host source/build·장치·identity·play/capture·peer final/drain·cleanup을 기록. C-stub typing이나 동일 Linux loopback은 교차 OS 실행 아님
  - [ ] **BK-WBS-12.13.02** 다중 host/device의 latency·clock offset/uncertainty·start/음향 분포를 반복 측정한다. — 상태=E(환경·장비대기); 선행=BK-WBS-12.13.01; 필요 근거/다음=실제 두 장치 이상에서 software schedule·원래 native output pair·loopback/acoustic onset·network probe를 함께 측정; remote prefix를 local clock으로 사용하지 않고 지연/변동/불확실성·반복 drift를 보고

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
  - [ ] **BK-WBS-13.02.01** UI·Business·Native IO의 실제 의존성과 composition root/test injection을 표로 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=REQ__performance-and-testability의 Layer boundary 및 current app 모듈을 대조해 hidden global·concrete adapter 잔여 경로를 표기한다. 코드 존재만으로 전체 layer 분리 완료를 선언하지 않는다.
  - [ ] **BK-WBS-13.02.02** core/BMS/Runtime의 pure port 테스트를 독립 invariant·실제 진입점에 대응한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=REQ__plan-acceptance-evidence의 각 소프트웨어 항목을 현재 test 이름·실행 명령·버전·미검증 branch에 연결하고 실제 coverage 누락을 남긴다.
  - [ ] **BK-WBS-13.02.03** native/browser adapter contract와 실제 장치·graphics·network acceptance를 분리한 coverage map을 완성한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=clock/input/output/storage/network/UI adapter의 원본 증거·오류 전파 테스트를 매핑하고 foreign typing·injected fixture·실제 실행을 별도 열로 표시한다.
- [ ] **BK-WBS-13.03** parser/compiler/input/replay/room codec에 지속 fuzz·corpus/minimization을 구축한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=REQ__layer-fuzz의 실제 sanitizer campaign·재현 artifact·minimization·오탐 방지 및 외부 지속 CI 실행.
  - [ ] **BK-WBS-13.03.01** 독립 fuzz workspace의 seed·raw-float·성공/거절 oracle 회귀를 실행한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=REQ__layer-fuzz 및 fuzz/tests/{codecs,bms,chart,seeds}.rs; `cargo test --manifest-path fuzz/Cargo.toml --no-default-features --locked`; 범위= 실제 NaN payload/±0·최대 Join·constant-tempo oracle와 create-new seed 보존을 확인한다.
  - [ ] **BK-WBS-13.03.02** physical input codec의 실제 ASan mutation campaign을 실행한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=`cargo +nightly-2026-10-08 fuzz build input_codec --fuzz-dir fuzz --features fuzzing --sanitizer address` 뒤 같은 target의 `fuzz run`; 범위= max_len65536·timeout2·RSS1024MiB·local10s·외부90s 제한과 원본 logs/corpus/artifacts를 보존한다.
  - [ ] **BK-WBS-13.03.03** replay codec의 nested-input/header/record 한계 ASan campaign을 실행한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=REQ__layer-fuzz의 replay_codec 실제 build/run; max_len65536·records64·header4096·원본 raw float bits fixed-point를 확인한다. Timeout을 PASS로 바꾸지 않는다.
  - [ ] **BK-WBS-13.03.04** room codec의 최대 Join·nested start·canonical bytes ASan campaign을 실행한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=room_codec 실제 ASan build/run; max_len131072와 실제 Join65808을 포함하고 성공 decode가 membership/authority를 부여하지 않음을 확인한다.
  - [ ] **BK-WBS-13.03.05** BMS text parser의 seeds0/73 및 compile families ASan campaign을 실행한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=bms_parser 실제 ASan build/run; max_len16384·256lines·1024bytes/line·256items에서 BGM/BGA/opacity/invisible/mine와 정당한 오류를 검사한다. Asset IO/legacy decoder까지 검증했다고 부르지 않는다.
  - [ ] **BK-WBS-13.03.06** structured chart compiler의 valid·overflow·negative-stop·duplicate 경계 ASan campaign을 실행한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=chart_compiler 실제 ASan build/run; max_len4096·18byte generator header·64objects·family별16markers와 독립 정수 기대를 검사한다.
  - [ ] **BK-WBS-13.03.07** 실제 corpus cmin·재실행과 controlled-failure artifact 재현/tmin을 증명한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=REQ__layer-fuzz의 실제 target corpus에 cmin 후 재실행하고 harness-probe의 controlled_failure로 original bytes·reproduction·tmin을 확인한다. 의도된 probe를 production 결함으로 집계하지 않는다.
  - [ ] **BK-WBS-13.03.08** 외부 지속 fuzz CI의 다섯 target 실제 실행과 실패 artifact 보존을 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=.github/workflows/fuzz.yml 선언과 별도로 hosted run URL/commit/tool versions·5개 결과·corpus cache·실패 시 original artifact 업로드를 확인한다. 외부 runner/access가 필요하다.
- [ ] **BK-WBS-13.04** 수집·queue·clock·output·network·UI에 deterministic fault/chaos schedule을 구축한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=입력/시계/queue의 18개 fixture 및 Runtime→Mixer mono/stereo PCM 거절·원래 owner 회수·실제 consumer 해제의 13개 scenario(19개 pair) 독립 code review/CLI QA 통과, [검증 지침](../verification/GUIDE__runtime-fault-schedules.md). [연결된 player network/UI 계약](../runtime/REQ__player-fault-isolation.md)의 7개 새 테스트로 실제 Runtime prefix·client/relay wire/ACK/drain·portable room controller UI 장애를 검사하고 치명 즉시 UI reply 오류 누락을 재현·수정; 최종 app 전체 2172 PASS/0 FAIL/6 ignored, 독립 code review·CLI QA·문서 검토 PASS(QA는 신규 7 및 기존 관련 67개 재실행). 실제 native output/network/UI·allocation/short write/spawn/panic 동시 경계는 남음.
  - [ ] **BK-WBS-13.04.01** Runtime의 input/audio clock 거절·queue saturation·watermark 보존 schedule을 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=GUIDE__runtime-fault-schedules의 cf3f110 독립 QA3tests/18fixtures는 기존 bounded proof다. current source에서 `cargo test -p beatkernel --test runtime_fault_schedule --locked`; 범위= 원본 metadata·intermediate input·FIFO를 재확인한다.
  - [ ] **BK-WBS-13.04.02** 실제 Runtime→Mixer mono/stereo PCM·거절·retirement 회수·consumer disconnect schedule을 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=같은 GUIDE의63963ce 독립 QA3tests/13scenario/19pairs가 scope proof다. `cargo test -p beatkernel --test runtime_output_fault_schedule --locked`; 범위= current 원본 PCM prefix/suffix·InvalidBuffer 무변경·owner 복구를 확인한다.
  - [ ] **BK-WBS-13.04.03** spawned collector의 마지막 FIFO/cut와 solo/cohort Runtime·capture 연결을 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=GUIDE의 bd8f881 독립 QA와 native_audio_authority41개·collector/startup18개. current app에서 native_input_fixtures/native_gameplay::fixtures::startup/native_gameplay::fixtures::same_collector 및 native_audio_authority 필터를 실행해 EOF와 실제 drained 완료를 구분한다.
  - [ ] **BK-WBS-13.04.04** portable client/relay wire·write receipt·ACK/drain과 RoomCompetition UI 실패 schedule을 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=REQ__player-fault-isolation의 실제 fatal immediate reply RED→수정→7tests 독립 QA를 보존한다. `cargo test -p beatkernel-bms-runtime --lib --features desktop,webtransport --locked fault_schedule_fixtures -- --nocapture`; 범위= current accepted Runtime prefix와 false completion 금지를 재확인한다.
  - [ ] **BK-WBS-13.04.05** allocation·short write·spawn·panic 동시 실패의 미검증 site와 oracle을 확정한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=위 GUIDE/REQ의 명시적 잔여 범위를 current production owner별로 감사하고 각 site에 실패 주입점·원본 error·보존 prefix·join/cleanup 기대를 배정한다. 기존 scoped fixture 수로 전체 coverage를 대체하지 않는다.
  - [ ] **BK-WBS-13.04.06** 실제 native output/network 및 desktop/browser route의 fault·cleanup 경계를 실행한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=실제 driver/socket/window/worker에서 중단·partial IO·device loss·spawn/panic·취소를 주입하고 원래 audio/input/network/UI owner와 join을 관측한다. injected portable source·ALSA null만으로 실제 하드웨어 완료를 선언하지 않는다.
- [ ] **BK-WBS-13.05** 대규모 노트·64-member·오랜 플레이·반복 seek/rebind의 soak/stress를 실행한다. — 상태=W(구현·연동중); 선행=-; 근거/다음=E33/E43 dense judge/replay/projector·자식 CPU/RSS report와 E44 실제64-member retained practice/pause/refusal·별도 admitted rebind software 검증 완료; E45 전체20h/week sparse1000Hz 가상 timeline 독립 code/security/QA 완료; 전체 player CPU/memory/queue/owner leak·표준-rate/native·장시간 soak는 계속 필요.
  - [x] **BK-WBS-13.05.01** 2만/10만 노트 dense judge·replay seek·indexed projector의 고정 software stress를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E33 및 REQ__dense-chart-stress의891eb85 독립 DEEP code/security/docs·CLI38건·core426/0. 실제2N기록·fresh-prefix oracle·narrow tail·원본 metadata·고정 storage를 검증했다. 해당 example/workload/test source는 current checkout까지 동일; 다음=`cargo test -p beatkernel --locked --test dense_chart_stress`; 범위=64lanes는64members나 wall-clock soak가 아니다.
  - [x] **BK-WBS-13.05.02** 실제 solo/cohort pump의1/2/3/4/64명 retained practice·pause·별도 admitted rebind software stress를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E44·REQ__cohort-transition-stress의1712136 독립 DEEP code/security 후 fresh11tests·release30CLI·반복64명 stdout동일. 해당 example/workload/test source는 current checkout까지 동일; 다음=`cargo test -p beatkernel-bms-runtime --no-default-features --locked --test cohort_transition_stress`; 범위=Practice 중 replacement 거절과 별도 허용 rebind를 구분한다.
  - [x] **BK-WBS-13.05.03** production solo pump의 sparse1000Hz 전체20h/week sample traversal과 pause/replay/capture를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E45·REQ__long-duration-player-traversal의a61b03a 독립 DEEP code/security 뒤 fresh6tests·release36CLI·실제72000000/604800000 successive frames·week반복동일·terminal queue비움. 해당 example/workload/test source는 current checkout까지 동일; 다음=`cargo run --release -p beatkernel-bms-runtime --no-default-features --locked --example long_duration_player -- --hours 20` 및 `cargo run --release -p beatkernel-bms-runtime --no-default-features --locked --example long_duration_player -- --hours 168`; 범위=native/표준-rate/실제 wall-clock soak나 task close 증거가 아니다.
  - [ ] **BK-WBS-13.05.04** 표준 sample-rate와 dense 실제 player의 seek/pause/rebind 반복 resource stress를 실행한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=현재 production launcher/owner에서44100/48000Hz·대형 chart·solo/local/online의 반복 seek/rebind를 고정하고 CPU/RSS·queue depth·retired owner·terminal drain을 기록한다.1000Hz sparse 또는64lanes 결과로 대신하지 않는다.
  - [ ] **BK-WBS-13.05.05** 실제 장시간 native/browser wall-clock soak와 owner/queue/memory 누수를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=지원 OS/browser·실제 clock/output/input·GPU/network를 고정해 장시간 실행하고 시작/steady/종료 CPU/RSS·queue/owner 수·xrun·cleanup raw data를 보존한다. 가상20h/week 순회를 실제 elapsed soak로 부르지 않는다.
- [ ] **BK-WBS-13.06** 전체 RT callback과 판정 hot path alloc/lock/I/O/error 경로를 계측한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=일반/오류/포화·모든 backend; scoped 테스트만 확대하지 않음.
  - [ ] **BK-WBS-13.06.01** 일반·거절·포화 judge/Runtime hot path의 allocation·lock·IO 경계를 계측한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 process_input/판정 경계에 calibrated allocator와 명시적 측정 구간을 연결해 normal/error/saturation·빈/큰 history·solo/cohort를 분리한다. gauge/publication scoped 결과를 전체 judge0alloc로 확대하지 않는다.
  - [ ] **BK-WBS-13.06.02** core Mixer의 render·pause/end·invalid/saturated/disconnected 경로 allocation을 확인한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=`cargo test -p beatkernel --locked --test audio_rt_alloc`; 범위=실제 alloc/realloc/dealloc calibration과 render 경계 밖 assertion을 확인한다. 이 검사는 OS callback의 lock/IO·native 제출/음향까지 증명하지 않는다.
  - [ ] **BK-WBS-13.06.03** WASAPI retained Source worker와 실제 Source/Target publication의 bounded allocation 경계를 확인한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E65/E70의 software scope와 crates/beatkernel-platform/tests/wasapi_source_worker_allocations.rs 및 app/tests/coordinate_{source,target}_replacement_allocations.rs를 current source로 재실행한다. original nonempty merger와 error/cold/refusal 경계를 포함한다.
  - [ ] **BK-WBS-13.06.04** ALSA·CoreAudio·ASIO와 converted Target의 실제 전체 callback 경계를 계측한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=현재 각 backend owner·변환·채널/geometry·최대 callback·실패/포화의 실제 계측 유무와 누락을 먼저 audit한다. CoreAudio retained Source는 진행 중이며 portable callback 실행/foreign typing은 실제 native callback 전체 검증이 아니다. 검증된 source/meter와 미구현 측정 지점을 구분해 갱신한다.
  - [ ] **BK-WBS-13.06.05** 모든 backend의 blocking lock·IO·panic/error·late cleanup 금지 site를 audit한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=실제 callback 진입부터 반환까지 call graph와 cold/preparation/retirement 경계를 기록하고 계측 누락·blocking 가능 지점에 근거를 붙인다. source 검색만으로 lock0/IO0를 보증하지 않는다.
  - [ ] **BK-WBS-13.06.06** 실제 OS callback의 일반·장치오류·포화 계측을 재현한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=실제 WASAPI/ASIO/ALSA/CoreAudio 장치와 tracing/allocator 관측으로 전체 callback을 실행한다. SDK/MSVC·macOS host·clock-capable output을 확보하고 기존 foreign typing/null 결과를 구분한다.
- [ ] **BK-WBS-13.07** p50/p95/p99/max·drop/xrun·scheduler jitter 계측을 재현 workload에 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=계측 위치/provenance/용량·관측 불가 표시.
  - [ ] **BK-WBS-13.07.01** bounded Runtime/Mixer p50/p95/p99/max를 현재 고정 software workload에 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E31·REQ__runtime-benchmark의6case×3회 report는 실제 baseline이다. 이후 runtime_bench audio_delta에 commands_expired가 추가돼 current release를 rebuild 후 tools/runtime_benchmark.py로 재실행한다. retained count/capacity·whole-child CPU와 loop wall time을 분리한다.
  - [ ] **BK-WBS-13.07.02** native input/render cadence의 원본 clock 위치·retained prefix·unavailable 오류를 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=platform audio/cadence.rs의4096point 고정 prefix·pre-Mixer QPC/mach/ALSA 위치·startup/prefill 포함 여부·회귀/overflow/unavailable을 actual example 출력과 대응한다. cadence를 callback-arrival/acoustic jitter로 바꾸지 않는다.
  - [ ] **BK-WBS-13.07.03** drop/xrun·queue 거절·inferred underrun·native 관측의 provenance를 구분한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=core/platform/app counters의 증가 위치·saturating 정책·관측 불가/null을 current workload summary와 대응한다. zero counter·unretained cadence·receipt gap을 실제 loss0/underrun0로 추정하지 않는다.
  - [ ] **BK-WBS-13.07.04** UI/network scheduler 지연과 제출 deadline의 재현 계측 경계를 확정한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=actual native/browser worker·UI submitted frame·room/network pump에서 schedule→service→submission의 clock/용량/누락을 감사해 고정 stall workload에 연결한다. 평균FPS나 synthetic jitter를 실제 scheduler tail로 표시하지 않는다.
- [ ] **BK-WBS-13.08** 성능 기준 workload·CPU/RAM/GPU/배포 artifact/실행 절차를 고정한다. — 상태=W(구현·연동중); 선행=-; 근거/다음=E31/E43 fixed runtime/dense software matrix의 CPU/RSS/artifact report 검증 완료; 전체 player 대형 chart·dense UI stall·다인·GPU·장시간 비교는 계속 필요.
  - [ ] **BK-WBS-13.08.01** 현재 runtime software baseline의6개 rate/buffer 조합과 원본 CPU/RSS/artifact를 고정한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=REQ__runtime-benchmark/E31; `cargo build --release -p beatkernel --example runtime_bench --locked` 후 `python3 tools/runtime_benchmark.py --binary target/release/examples/runtime_bench`; 범위=44100/48000×32/128/512·3repeats·source/tool/CPU identity·unavailable를 검증한다. 이후 source delta가 있으므로 과거 PASS만으로 current D를 주지 않는다.
  - [x] **BK-WBS-13.08.02** 2만/10만 dense software matrix의 원본 child CPU/RSS와 artifact report를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E43·REQ__dense-chart-stress의87c8ba9/531877c 독립 DEEP code/security·CLI/docs 및 Harness close, Python44·fresh release·2case×3회·추가 stdout/거절 검사. collector/example/workload/test source는 current checkout까지 동일; 다음=`python3 tools/dense_chart_benchmark.py --binary target/release/examples/dense_chart_stress`; 범위=837856bytes는 benchmark이며 player크기·GPU·64members·soak 결과가 아니다.
  - [ ] **BK-WBS-13.08.03** 전체 player의 대형 chart·solo/local/online·dense UI stall workload와 CPU/RAM 기준을 고정한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=실제 launcher/scene/worker와 selected chart/media·member수·report/history·seek/rebind·network stall을 고정하고 source/build/host/raw repeat 관측을 남긴다. 독립 oracle 검증 시간을 gameplay 측정과 분리한다.
  - [ ] **BK-WBS-13.08.04** native/browser cold·warm startup/전환/선택 준비의 반복 latency 기준을 고정한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=REQ__performance-and-testability의 loading 계약·app/web/tests/loading-latency.mjs·tools/loading_report.py를 사용해 baseline/current 전체 dependency tree·cache owner/hash를 고정한다. 최소5software trials의 first-selectable·실제 usable frame·parse/decode/prepare/output-ready p50/p95/max를 분리한다.
  - [ ] **BK-WBS-13.08.05** 실제 GPU/frame deadline·업로드와 장시간 resource 기준을 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=고정 CPU/RAM/GPU/driver/display/browser/build·대형 chart·다인·stall/seek·장시간 조건에서 actual presented frame·upload·CPU/RSS·queue/owner를 측정한다. SwiftShader/llvmpipe correctness를 hardware 성능으로 승격하지 않는다.
  - [ ] **BK-WBS-13.08.06** 실제 player 배포 artifact의 크기·feature·source/build identity를 고정한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=non-ASIO와ASIO·OS/browser별 실제 candidate package·압축 전후 byte크기·SHA256·compiler/feature/dependency를 기록한다. runtime/dense benchmark executable 크기를 player 배포 크기로 재사용하지 않는다.
- [ ] **BK-WBS-13.09** 입력 1kHz 이상에서 device→collector→runtime 손실/순서/jitter를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=실제 source/device.
  - [ ] **BK-WBS-13.09.01** 1kHz 이상 실제 source의 emitted sequence·시계·driver/설정 manifest를 확정한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=source가 실제 낸 edge/count/sequence와 장치/OS/driver·nominal cadence·native/receipt/runtime clock 위치를 기록한다. source가 없는 상태에서 receipt 간격만으로 정확한 loss count를 만들지 않는다.
  - [ ] **BK-WBS-13.09.02** Windows 실제 Raw Input/HID 및 지원 input API의1kHz acquisition을 측정한다. — 상태=E(환경·장비대기); 선행=BK-WBS-13.09.01; 필요 근거/다음=실제 Windows source/device를 선택한 뒤 `cargo run -p beatkernel-platform --example windows_input_inspector --locked -- --seconds 60 --cadence DEVICE_ID HID_KEY_USAGE down 1000000`을 실제 source/device 값으로 실행하고 신호에 따라 down/up/repeat를 선택한다. API별 inspector도 실행한다; 범위=backend별 provenance·손실/순서·jitter raw data를 source sequence와 대조한다.
  - [ ] **BK-WBS-13.09.03** Linux 실제 evdev/hidraw의1kHz acquisition을 측정한다. — 상태=E(환경·장비대기); 선행=BK-WBS-13.09.01; 필요 근거/다음=`cargo run -p beatkernel-platform --example linux_input_cadence --locked -- evdev PATH HID_KEYBOARD_USAGE down 1000000 60` 또는 `cargo run -p beatkernel-platform --example linux_input_cadence --locked -- hidraw PATH none 1000000 60`을 실제 source/device 값으로 실행한다. 신호에 따라 down/up/repeat 및 none/REPORT_ID를 선택한다; 범위=evdev CLOCK_MONOTONIC과 hidraw userspace receipt를 구분하고 source sequence로 손실/순서를 확인한다.
  - [ ] **BK-WBS-13.09.04** macOS 실제 IOHID source의1kHz native timestamp/receipt age를 측정한다. — 상태=E(환경·장비대기); 선행=BK-WBS-13.09.01; 필요 근거/다음=`cargo run -p beatkernel-platform --example macos_input_cadence --locked -- --list` 뒤 `cargo run -p beatkernel-platform --example macos_input_cadence --locked -- REGISTRY_ENTRY ELEMENT_COOKIE USAGE_PAGE USAGE INTEGER_VALUE 1000000 60`을 실제 source/device 값으로 실행한다; 범위=실제 device·Input Monitoring 권한·native mach 원본과 receipt age가 필요하다.
  - [ ] **BK-WBS-13.09.05** 실제 device→spawned collector→Runtime의 bounded 전달·원본 순서·jitter를 검증한다. — 상태=E(환경·장비대기); 선행=BK-WBS-13.09.01; 필요 근거/다음=실제 launcher에서 동일 source sequence를 canonical metadata·collector FIFO/cut·runtime/capture까지 추적하고 queue 포화/terminal drain을 포함한다. inspector cadence나 injected collector만으로 종단 전달 완료를 선언하지 않는다.
- [ ] **BK-WBS-13.10** buffer 크기별 native audio underrun/latency·교체 후 지연을 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=실제 WASAPI/ASIO/ALSA/CoreAudio.
  - [ ] **BK-WBS-13.10.01** WASAPI shared의 실제 buffer/period별 underrun·latency·render cadence를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=`cargo run -p beatkernel-platform --example windows_audio --locked -- --list` 후 명시적 device의 `--mode shared --play`와 실제 `--rate`, `--channels`, `--buffer`, `--period`, `--seconds` 조합을 실행한다; 범위=actual applied metadata·QPC render-entry와 native/presentation/physical 측정을 분리한다.
  - [ ] **BK-WBS-13.10.02** WASAPI exclusive의 실제 geometry와 buffer별 underrun·latency를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=`cargo run -p beatkernel-platform --example windows_audio --locked -- --help`로 실제 `--probe`/`--play` 경로를 확인한 뒤 `--mode exclusive`와 실제 지원 형식·동일 buffer/period를 사용한다; 범위=실제 GetBufferSize·event/prefill·MMCSS·whole-lease/복구 multiple을 포함하며 E70의9개 native fixture는 미실행 상태다.
  - [ ] **BK-WBS-13.10.03** SDK-enabled ASIO의 실제 driver/buffer callback latency·underrun을 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=REQ__asio-stream/driver-control의 caller-supplied SDK·MSVC·명시적 CLSID와 실제 장치로 실행한다. actual applied buffer와 원본 interval evidence를 기록하고 foreign typing/SDK-free 거절을 출력 증거로 쓰지 않는다.
  - [ ] **BK-WBS-13.10.04** ALSA의 실제 sample-rate/period/buffer별 xrun·latency를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=`cargo run -p beatkernel-platform --example linux_native --locked -- audio ALSA_ENDPOINT RATE CHANNELS PERIOD_FRAMES BUFFER_FRAMES SECONDS`와 actual launcher를 실제 값의 clock-capable endpoint에 연결한다; 범위=snd_pcm 실제 status/delay·applied geometry를 보존한다. null PREPARED/played-frame없음을 physical timing으로 대체하지 않는다.
  - [ ] **BK-WBS-13.10.05** CoreAudio의 실제 장치/최대 callback/buffer별 latency를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=owned macOS host·CoreAudio device에서 macos_native와 actual solo/local/replay owner를 실행한다. actual callback maximum·mach cadence·presentation/latency·stop/retirement를 기록한다. 현재 owned Mac 부재를 보존한다.
  - [ ] **BK-WBS-13.10.06** 각 실제 backend의 교체·pause/resume 뒤 지연과 원본 owner 연속성을 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=same-rate/converted Source·Target·실제 buffer/rate/device 변경에서 이전/새 driver의 관측·remaining PCM·first audible/current frame·cleanup을 원본 metadata와 대조한다. portable replacement correctness를 음향 continuity/latency로 승격하지 않는다.
- [ ] **BK-WBS-13.11** 입력→소리·구간 재시작의 end-to-end 음향 latency/drift를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=물리 loopback/측정장비와 raw data.
  - [ ] **BK-WBS-13.11.01** 실제 input→소리의 end-to-end p50/p95/p99/max와 clock uncertainty를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=실제 input pulse/loopback/측정 장비·동기화/케이블 지연 보정·source/build/device/buffer를 고정해 raw waveforms/edge timestamps·실패와 outlier를 보존한다. 소프트웨어 processing 시간으로 음향 latency를 대신하지 않는다.
  - [ ] **BK-WBS-13.11.02** 실제 구간 재시작·반복·출력 교체의 음향 gap/latency/drift를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=known PCM cue와 실제 acoustic/loopback capture를 사용해 restart/pause/rebind 전후 원래 source시간·target frame·관측 clock과 waveforms를 대조한다. idle/silent frame·converted ratio·장시간 drift와 측정 uncertainty를 명시한다.
- [ ] **BK-WBS-13.12** 동일 조건에서 경쟁 플레이어와 CPU/memory/latency를 비교해 목표 성능을 검증한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=동일 workload/device/build; osu! 대비 우위 추정 금지.
  - [ ] **BK-WBS-13.12.01** 경쟁 비교의 동일 chart/media·device·buffer/display·build/version 조건을 고정한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=비교 대상별 실제 version/feature/latency 설정·workload checksum·cold/warm policy·CPU power/driver/display를 기록하고 불일치/미지원 조건을 명시한다. osu! 대비 우위나 세계 순위를 추정하지 않는다.
  - [ ] **BK-WBS-13.12.02** 고정 조건에서 실제 CPU/RSS/GPU·배포크기·startup/steady workload를 반복 비교한다. — 상태=E(환경·장비대기); 선행=BK-WBS-13.12.01; 필요 근거/다음=동일 실제 host와 matched workload로 여러 회 원본 process/resource/frame 관측을 수집한다. 단일 평균·benchmark executable·서로 다른 workload 수치를 비교하지 않는다.
  - [ ] **BK-WBS-13.12.03** 고정 조건에서 실제 input-to-audio tail·underrun·drift를 비교한다. — 상태=E(환경·장비대기); 선행=BK-WBS-13.12.01; 필요 근거/다음=동일 source/device/buffer·physical apparatus/calibration·raw waveform으로 latency distribution/실패/uncertainty를 측정한다. 측정된 조건의 결론만 보고하고 우위 미달/비교 불가도 그대로 기록한다.
- [ ] **BK-WBS-13.13** 안정성 목표의 coverage/soak/fuzz/결정론 한계와 재현 실패를 기록한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=SQLite급이라는 표현을 테스트 수/소스 존재로 보증하지 않음.
  - [ ] **BK-WBS-13.13.01** 원본§16–17·layer별 transition/branch/invariant coverage와 누락을 기록한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=REQ__performance-and-testability·REQ__plan-acceptance-evidence와13.02 coverage map을 실제 test/adapter/run/commit에 연결한다. source 존재/test개수로 신뢰성 수준을 보증하지 않는다.
  - [ ] **BK-WBS-13.13.02** fuzz 원본 실패·최소 재현·corpus·도구/버전·오탐 경계를 기록한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=13.03의 실제 campaign 결과와 미실행/환경 실패를 구분해 seed/raw input/minimized input·재현 argv·한계를 남긴다. bounded finite corpus와 지속 sanitizer campaign을 분리한다.
  - [ ] **BK-WBS-13.13.03** dense/cohort/virtual-long/실제 soak·RT·물리 검증의 한계와 재현 실패를 기록한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=E31/E33/E43/E44/E45 및13.05–13.12 결과를 software/standard-rate/native/GPU/network/wall-clock/physical로 정확히 구분하고 failed attempt를 보존한다. SQLite급/세계최고 표현을 PASS수로 보증하지 않는다.
  - [ ] **BK-WBS-13.13.04** 현재 checkout의 증거 유효성·변경된 scope·재실행 필요 항목을 감사한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=original source/build/artifact identity와 current diff·owner/lifecycle 변경을 대조한다. 무효 증거의 leaf를 V/W로 되돌리고 다음 검사를 명시하며 남은 범위를 전체완료로 축약하지 않는다.

### 14. 빌드·독립 검증·문서·라이선스·출시

기능 연결: BK-054, BK-055, BK-056, BK-057, BK-058, BK-059. 요구: plan §§18–24; MIT/ASIO distribution.

- [x] **BK-WBS-14.01** 전체 WBS의 stable ID·완료 기준·근거·변경 이력과 자동 집계 기준을 구축한다. — 상태=D(검증완료); 선행=-; 근거=E10; 이후 구현자가 관련 leaf와 증거를 갱신한다.
- [ ] **BK-WBS-14.02** 현재 workspace/lib/bin/example/doctest의 debug/release 회귀를 실행한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=2011 lib만 전체 workspace 결과로 부르지 않음.
  - [ ] **BK-WBS-14.02.01** 현재 workspace 전체 debug lib/bin/integration/target 회귀를 실행한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=`cargo test --workspace --all-targets --locked`를 frozen combined source에서 실행한다; 범위= crate/target별 argv·exit·pass/fail/ignored·build identity를 보존한다. E74의 complete core/platform/app bounded QA나2011lib만으로 새 workspace PASS를 선언하지 않는다.
  - [ ] **BK-WBS-14.02.02** 현재 workspace 전체 optimized release 회귀를 실행한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=`cargo test --workspace --all-targets --release --locked`를 같은 frozen source에서 실행한다; 범위= scoped release focused/benchmark build를 전체 release suite로 부르지 않는다.
  - [ ] **BK-WBS-14.02.03** 현재 workspace rustdoc 예제의 debug/release 실행을 확인한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=`cargo test --workspace --doc --locked` 및 `cargo test --workspace --release --doc --locked`를 실행한다; 범위=`--all-targets`와 별도 doctest 결과를 기록하고 compile-only와 실제 실행을 구분한다.
  - [ ] **BK-WBS-14.02.04** 현재 공개 example/CLI의 정상·거절·fixture 경로를 debug/release에서 실행한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=.github/workflows/ci.yml의 `cargo run -p beatkernel --example transport --locked`, `cargo run -p beatkernel --example binding --locked`, `cargo run -p beatkernel --example audio --locked -- --fixture`, `cargo run -p beatkernel-platform --example windows_audio --locked -- --fixture`, `cargo run -p beatkernel-platform --example input_inspector --locked` 등과 실제 일반화 example을 debug/release에서 실행한다; 범위=도움말/오류/정상 argv·exit·결과를 확인하고 native device 모드는 실제 장치 evidence와 구분한다.
- [ ] **BK-WBS-14.03** Windows/macOS foreign typing과 실제 native build를 구분해 검사한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=C/archive stub은 SDK/device 실행 증거 아님.
  - [ ] **BK-WBS-14.03.01** Windows target의 app/platform lib/tests foreign Rust typing을 재현한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=기존 C/archive stub recipe·target triple·feature·source identity로 Rust typing을 검사하고 stub 범위와 미실행 native fixture를 명시한다. ABI/SDK/COM/MMCSS/driver build/runtime PASS로 부르지 않는다.
  - [ ] **BK-WBS-14.03.02** macOS target의 app/platform lib/tests foreign Rust typing을 재현한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=기존 framework/C-stub recipe·target triple·feature·source identity로 Rust typing을 검사한다. CoreAudio/IOHID native framework link·ABI·callback/device 실행은 별도다.
  - [ ] **BK-WBS-14.03.03** Windows 실제 MSVC/native SDK의 non-ASIO·선택 ASIO 빌드를 실행한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=실제 Windows build host·MSVC/Windows SDK·ASIO 포함 시 caller-supplied SDK로 native app/platform binary와 tests를 build한다. VM endpoint0·로그인 콘솔/현재 player/build tool 부재 기록을 보존한다.
  - [ ] **BK-WBS-14.03.04** macOS 실제 SDK/framework의 native app/platform 빌드를 실행한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=owned macOS host와 실제 SDK/framework에서 app/platform bin/test를 build한다. current owned Mac 부재·macos-vm host없음을 해소해야 하며 C-stub typing으로 대신하지 않는다.
- [ ] **BK-WBS-14.04** WASM/headless/desktop/선택 feature별 현재 build와 shader를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 pkg 재생성·GPU pipeline.
  - [ ] **BK-WBS-14.04.01** headless/model의 native graphics/browser 미의존 build를 검사한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=`cargo check -p beatkernel-bms-runtime --no-default-features --lib --locked`; 범위= 실제 dependency graph를 확인한다. E9/이전 model check를 current full feature matrix로 부르지 않는다.
  - [ ] **BK-WBS-14.04.02** host desktop·webtransport의 current lib/bin/example build를 검사한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=`cargo check -p beatkernel-bms-runtime --features desktop,webtransport --all-targets --locked` 및 실제 release candidate build를 수행한다; 범위=OS별 native link/실행은14.03/14.12에 남긴다.
  - [ ] **BK-WBS-14.04.03** 현재 browser main WASM과 정확한 wasm-bindgen/export package를 재생성한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=`cargo build -p beatkernel-bms-runtime --lib --no-default-features --features browser --target wasm32-unknown-unknown --release --locked` 뒤 pinned wasm-bindgen0.2.129로 실제 package를 만든다; 범위=source/WASM/JS SHA와 exports·BigInt 경계를 보존한다. 현재=doc/changes/2026-10-15-browser-audio-current-acceptance.md의 별도 offline build→즉시 bindgen 두 패키지와 실제20파일 snapshot/해시를 확인했으나 strict browser acceptance는 BLOCKED_ENV; V 유지.
  - [ ] **BK-WBS-14.04.04** 현재 browser-audio WASM/AudioWorklet의 package와 dependency closure를 재생성한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=browser-audio feature를 별도 WASM build/bindgen하고 실제 worker/worklet JS+WASM dependency tree를 current build로 고정한다. main package만 재생성한 결과로 오디오 package를 대체하지 않는다. 현재=doc/changes/2026-10-15-browser-audio-current-acceptance.md의 별도 offline build→즉시 bindgen 두 패키지와 실제20파일 snapshot/해시를 확인했으나 strict browser acceptance는 BLOCKED_ENV; V 유지.
  - [ ] **BK-WBS-14.04.05** 현재 native/browser GPU shader·pipeline·제출/재생성 경로를 실행한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-14.04.02, BK-WBS-14.04.03; 필요 근거/다음=GUIDE__headless-webgpu와 실제 native draw/worker OffscreenCanvas에서 shader validation·pipeline 생성·texture/BGA·resize/recovery/dispose를 확인한다. software GPU correctness와 실제 hardware perf는 분리한다.
- [ ] **BK-WBS-14.05** fmt/clippy/CI의 current 회귀와 기존 실패를 구분하고 정식 matrix를 통과한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=hosted CI·native runner; user mise.toml 변경 금지.
  - [ ] **BK-WBS-14.05.01** 전체 current Rust formatting 결과와 기존 source debt를 구분한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=`cargo fmt --all -- --check`를 frozen source에서 실행한다; 범위= 전체 결과와 기존/신규 실패 파일을 구분한다. scoped Rust15/23 PASS를 whole-workspace formatting으로 승격하지 않는다.
  - [ ] **BK-WBS-14.05.02** 전체 strict Clippy matrix의 warning/failure를 분류하고 필요한 회귀를 해소한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=`cargo clippy --workspace --all-targets --locked -- -D warnings`의 실제 current 결과를 보존한다; 범위= E74 non-blocking app394/platform111 warnings exit0는 strict clean이 아니며 inherited와 신규 원인을 나눈다.
  - [ ] **BK-WBS-14.05.03** Windows/macOS/Linux 정식 hosted CI의 실제 matrix 결과를 확인한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=.github/workflows/ci.yml의 os3개·fmt/strict Clippy/debug/release/examples/rustdoc actual run URL·commit·logs·failure를 확인한다. YAML선언·foreign typing·local Linux만으로 hosted matrix PASS를 주지 않는다. user mise.toml은 읽거나 수정하지 않는다.
- [ ] **BK-WBS-14.06** 공개 API/examples/rustdoc와 실제 실행·오류·설정/재시작 문서를 완성한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=원본 phase0–14/일반화 fixture usage.
  - [ ] **BK-WBS-14.06.01** 공개 API·crate-level rustdoc·예제 문서의 current signature/contract를 완성한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=`cargo doc --workspace --no-deps --locked`; 범위= actual public API/examples/doctests를 대조한다. owner/limits/errors/port injection·사용법이 구현과 일치해야 한다.
  - [ ] **BK-WBS-14.06.02** 원본 Phase0–14·일반화 fixture의 실제 실행/거절과 native 조건을 문서화한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=plan§18/§22의 actual example/CLI argv·정상/오류 결과·canonical device/source provenance·foreign typing 한계·장치/권한 조건을 current source에 연결한다. test source 존재나 Rust API를 SDK host 완료로 쓰지 않는다.
  - [ ] **BK-WBS-14.06.03** 실제 player의 설정·시작/종료·재시작·pause/rebind·browser 권한/오류 회복 지침을 완성한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=현재 native/browser 실행 경로와 제한·재시작/실패/cleanup을 재현 가능한 사용자 절차로 검토한다. 지원하지 않는 finite/network/practice 조합을 성공 절차로 쓰지 않는다.
- [ ] **BK-WBS-14.07** 전체 변경에 독립 code/security/document review를 완료한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=actual structural PASS; 현재 task PENDING.
  - [ ] **BK-WBS-14.07.01** frozen combined source의 독립 code review와 수정 후 최종 PASS를 확보한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=현재 전체 변경/원본 요구 범위를 실제 독립 reviewer에 전달하고 structural final PASS·발견/수정·최종 frozen identity를 보존한다. 부분 child 리뷰를 전체 변경 리뷰로 확대하지 않는다.
  - [ ] **BK-WBS-14.07.02** frozen combined source의 독립 security review를 완료한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 input/file/archive/network/TLS/Origin/resource/native boundary를 scope에 넣고 applicable security lens의 최종 structural PASS를 확인한다. advisory/시작 기록/이전 snapshot은 대체 근거가 아니다.
  - [ ] **BK-WBS-14.07.03** current 요구·변경·사용 문서의 독립 문서 review를 완료한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=full WBS/source/evidence/링크/미완료·환경 조건을 frozen source와 대조한 independent documentation review의 실제 최종 PASS를 확보한다.
  - [ ] **BK-WBS-14.07.04** review 완료 기록이 owning task의 실제 ordered receipt와 일치하는지 확인한다. — 상태=U(현황감사필요); 선행=BK-WBS-14.07.01, BK-WBS-14.07.02, BK-WBS-14.07.03; 필요 근거/다음=actual required reviewer/lens의 hook-owned 완료와 final structural 결과·run/task identity를 확인한다. coordinator 서술·direct NON-ATTESTING 결과·start-only/PENDING은 close PASS를 주지 않는다.
- [ ] **BK-WBS-14.08** review PASS 뒤 독립 CLI/browser/desktop QA를 실제 환경에서 완료한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=qa-browser 필수; start receipt≠PASS.
  - [ ] **BK-WBS-14.08.01** 최종 review PASS 뒤 frozen source의 독립 CLI QA를 실행한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-14.07.04; 필요 근거/다음=actual CLI lens가 complete software suite·공개 정상/거절 경로·실제 PCM/result·현재 build identity·cleanup을 검사하고 structural PASS를 내야 한다. previous child QA를 full combined current QA로 부르지 않는다.
  - [ ] **BK-WBS-14.08.02** 최종 review PASS 뒤 현재 WASM/package의 독립 browser QA를 실행한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-14.07.04, BK-WBS-14.04.03, BK-WBS-14.04.04; 필요 근거/다음=qa-browser 필수. 실제 browser/worker/worklet/GPU와 library→prepare→play/local→capture→records/replay→settings/restart/실패/cleanup을 검사하고 screenshot/raw evidence를 보존한다. Node나 WASM compile은 실제 QA를 대신하지 않는다.
  - [ ] **BK-WBS-14.08.03** 최종 review PASS 뒤 지원 native OS의 독립 desktop QA를 실행한다. — 상태=E(환경·장비대기); 선행=BK-WBS-14.07.04; 필요 근거/다음=실제 current binary/window/GPU·입력·clock-capable audio와 native stop/restart/device loss를 검사한다. Windows endpoint0/로그인 콘솔·Mac host 부재 등 actual blocker를 유지하며 stub/X11 fixture만으로 전체 native PASS를 주지 않는다.
  - [ ] **BK-WBS-14.08.04** QA별 actual structural PASS와 review 이후 hook-owned 완료 순서를 확인한다. — 상태=U(현황감사필요); 선행=BK-WBS-14.08.01, BK-WBS-14.08.02, BK-WBS-14.08.03; 필요 근거/다음=required CLI/browser/desktop final receipt가 actual review PASS 뒤에 생겼는지 owning task_verify로 확인한다. start receipt·direct final·BLOCKED_ENV·PENDING을 완료로 바꾸지 않는다.
- [x] **BK-WBS-14.09** MIT source·ASIO 제외 MIT/포함 GPLv3 정책을 문서·root license에 기록한다. — 상태=D(검증완료); 선행=-; 근거=E7.
- [ ] **BK-WBS-14.10** 실제 non-ASIO release의 third-party notice/feature/dependency/artifact를 감사한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=MIT 조건·코덱/폰트/GPU 의존성.
  - [ ] **BK-WBS-14.10.01** non-ASIO candidate의 실제 feature/dependency closure와 source/build identity를 고정한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 native/browser release별 Cargo.lock·compiler·feature tree·OS/target·build argv·source identity를 보존한다. `cargo tree -p beatkernel-bms-runtime --features desktop,webtransport --locked -e features` 등을 실제 artifact 설정에 맞춰 확인한다; 범위=해당 release의 feature/dependency closure.
  - [ ] **BK-WBS-14.10.02** 실제 포함 codec/font/GPU/window 등 third-party notice와 라이선스를 감사한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=REQ__asio-distribution의 non-ASIO MIT조건과 각 포함 원본 component notice/라이선스·동적/배포 resource를 실제 closure에 대조한다. root MIT metadata만으로 배포 감사를 완료하지 않는다.
  - [ ] **BK-WBS-14.10.03** non-ASIO 실제 candidate package와 checksums/build provenance를 만든다. — 상태=U(현황감사필요); 선행=BK-WBS-14.10.01, BK-WBS-14.10.02; 필요 근거/다음=지원 OS/browser별 current executable·JS/WASM·필수 resources·licenses/notices를 담은 실제 package의 파일 목록·SHA256·크기·정확한 build/source identity를 기록한다. 정책 문서나 benchmark binary를 release artifact로 부르지 않는다.
  - [ ] **BK-WBS-14.10.04** 패키지에 해당 notice/feature/artifact가 실제 포함됐는지 최종 감사한다. — 상태=U(현황감사필요); 선행=BK-WBS-14.10.03; 필요 근거/다음=새 package를 독립적으로 열어 non-ASIO feature 미포함·MIT/third-party notice·resource closure·unintended SDK/code 포함 여부를 실제 파일/빌드 결과와 대조한다.
- [ ] **BK-WBS-14.11** ASIO combined release의 SDK notice/GPLv3/Corresponding Source를 감사한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 MSVC binary·재현 source/build bundle.
  - [ ] **BK-WBS-14.11.01** ASIO 실제 SDK revision/원본 license·patch/generated/build 입력을 확보한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=caller-supplied SDK 원본·정확한 revision·embedded notices·patches·generated source와 MSVC/Windows SDK 입력을 보존한다. SDK파일을 MIT로 재표기하지 않는다.
  - [ ] **BK-WBS-14.11.02** SDK-enabled 실제 MSVC combined release를 build하고 artifact identity를 고정한다. — 상태=E(환경·장비대기); 선행=BK-WBS-14.11.01; 필요 근거/다음=실제 Windows+MSVC·명시적 asio-sdk feature·supplied SDK로 build해 binary/feature/dependency/source/build manifest·SHA256를 보존한다. foreign C-stub typing이나 SDK-free refusal는 실제 combined build 증거가 아니다.
  - [ ] **BK-WBS-14.11.03** 실제 combined package의 GPLv3 표시·전문·MIT/SDK/component notice를 감사한다. — 상태=U(현황감사필요); 선행=BK-WBS-14.11.02; 필요 근거/다음=REQ__asio-distribution의 selected GPLv3조건과 실제 combined release 파일/표시/각 embedded notices를 대조한다. 기존 정책14.09 D를 실제 release license 감사 완료로 재사용하지 않는다.
  - [ ] **BK-WBS-14.11.04** 해당 binary의 Corresponding Source와 필요한 build/source bundle을 재현 감사한다. — 상태=U(현황감사필요); 선행=BK-WBS-14.11.02; 필요 근거/다음=정확한 project/SDK revision·patch/generated source·필요한 build/config scripts/instructions를 해당 binary와 함께 전달 가능한 실제 bundle로 확인한다. upstream URL이나 SDK폴더만을 combined program의 source로 부르지 않는다.
- [ ] **BK-WBS-14.12** 지원 OS/browser의 설치·기본 시작·실제 입력/오디오·upgrade/failure smoke를 완료한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=플랫폼 release matrix.
  - [ ] **BK-WBS-14.12.01** Linux 실제 release 설치·기본 시작·evdev/hidraw/ALSA·upgrade/failure smoke를 실행한다. — 상태=E(환경·장비대기); 선행=BK-WBS-14.10.03; 필요 근거/다음=실제 packaged current binary를 깨끗한 지원 Linux 환경에 설치해 명시적 실제 input/clock-capable output·permissions·settings/restart·실패 후 cleanup·기존 기록 보존을 확인한다. null/CLI help만으로 대신하지 않는다.
  - [ ] **BK-WBS-14.12.02** Windows 실제 release 설치·기본 시작·Raw Input/HID/WASAPI·upgrade/failure smoke를 실행한다. — 상태=E(환경·장비대기); 선행=BK-WBS-14.10.03; 필요 근거/다음=실제 로그인 desktop·render endpoint·current installed binary에서 native input/audio·settings/restart·실패/upgrade와 자료 보존을 검사한다. ASIO package는 별도 actual SDK/driver 조건으로 검사한다.
  - [ ] **BK-WBS-14.12.03** macOS 실제 release 설치·기본 시작·IOHID/CoreAudio·upgrade/failure smoke를 실행한다. — 상태=E(환경·장비대기); 선행=BK-WBS-14.10.03; 필요 근거/다음=owned supported Mac과 actual package·권한/OS 보안 prompt·input/audio device로 install/start/restart/upgrade/failure를 검사한다. foreign typing/미사용 macos-vm 선언으로 대신하지 않는다.
  - [ ] **BK-WBS-14.12.04** 지원 browser의 실제 배포 package·권한/입력/오디오·upgrade/failure smoke를 실행한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-14.04.03, BK-WBS-14.04.04; 필요 근거/다음=선언한 지원 browser/version별 실제 served package·Worker/AudioWorklet·file/touch/keyboard·audio resume·IndexedDB/history·reload/upgrade/cancel/failure·owner cleanup을 검사한다. Chromium 한 번이나 Node unit PASS를 전체 지원 matrix로 부르지 않는다.
- [ ] **BK-WBS-14.13** 원본 plan §24와 모든 active WBS를 증거별로 확인한 뒤 최종 release를 만든다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=미완료/조건부를 숨기지 않는 Goal completion audit.
  - [ ] **BK-WBS-14.13.01** 원본 plan§24의 각1.0 후보 조건을 actual source/실행/물리 증거로 감사한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=§24의 아키텍처·Windows+한 플랫폼 native input/audio·canonical/binding/raw HID·5객체/6입력·transport/replay/seek·RT금지·4종 fixture·telemetry/API docs를 개별 증거에 대응한다. 최소 native 조건으로 이후 추가된 전체 player/WASM/UI/멀티 요구를 지우지 않는다.
  - [ ] **BK-WBS-14.13.02** 원래193 active 요구와 모든 확장 leaf의 증거·조건부·미완료를 최종 대조한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=`python3 tools/wbs_status.py`, `python3 tools/wbs_status.py --json`, `python3 tools/wbs_status.py --ready`, `python3 tools/wbs_status.py --in-progress`; 범위= 원본/제품 요구를 대조해 active 미완료·E/P/U·조건부·무효 증거를 숨기지 않는다. 부모 D는 전체 활성 말단+원래 범위와 일치해야 하며 checklist%를 출시 준비도로 부르지 않는다.
  - [ ] **BK-WBS-14.13.03** 최종 검증된 revision의 실제 release package와 manifest를 확정한다. — 상태=U(현황감사필요); 선행=BK-WBS-14.13.01, BK-WBS-14.13.02, BK-WBS-14.08, BK-WBS-14.10, BK-WBS-14.11, BK-WBS-14.12; 필요 근거/다음=모든 활성 요구의 실제 완료와 release license/install QA를 확인한 동일 source/build/artifact를 freeze하고 실제 package·SHA256·지원 matrix·known limits/release notes를 만든다. 미완료 상태에서 release/전체 Goal 완료를 선언하지 않는다.
  - [ ] **BK-WBS-14.13.04** owning task/Goal의 실제 ordered verification과 release completion evidence를 확인한다. — 상태=U(현황감사필요); 선행=BK-WBS-14.13.03; 필요 근거/다음=final structural review/QA와 hook-owned completion 뒤 task_verify/close 및 전체 Goal의 모든 원래 조건을 대조한다. NON-ATTESTING 결과·PENDING/환경대기는 제품/Goal 완료가 아니다.

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

현재 focused task는 `TASK__retained-source-coreaudio-output`이다. 원래
`BK-WBS-07.08.07.15`의 범위를 아래12개 실행 단위로 추적한다. 구현된 portable
기반의 실제 검사와 아직 실행하지 못한 native fixture·장비 증거를 구분한다.

| 현재 CoreAudio task AC | 안정적인 실행 단위 | 현재 근거/다음 검사 |
| --- | --- | --- |
| AC002 | BK-WBS-07.08.07.15.01 | D; 공통 승인 CAS 실제10개, 전체 platform450 내부 |
| AC010 | BK-WBS-07.08.07.15.02 | D; core history14·platform output state21 실제 통과 |
| AC003 | BK-WBS-07.08.07.15.03 | D; portable production callback 실제19개 통과 |
| AC004 native 소유권 | BK-WBS-07.08.07.15.04 | V; b8f7494f, 승인 후 관측 fence·실제 maximum 형 검사 통과 |
| AC004 실패 경계 | BK-WBS-07.08.07.15.05 | V; native audio52개·추가 presentation6개 typed-only, 실제 실행 남음 |
| AC005 | BK-WBS-07.08.07.15.06 | V; AC012 future 관측·authorization 경합 수정, app3279/0/ignored8·macOS typing 및 정식 code/security review·독립 CLI software QA PASS; cfg-Mac backend fixture 실행·실제 Mac acceptance 남음 |
| AC006 | BK-WBS-07.08.07.15.07 | V; 83b50487 typed UI 연결·macOS typing 및 공통 request9 통과 |
| AC007 | BK-WBS-07.08.07.15.08 | V; 9e68bbb4 complete solo Source pump·cold 준비 host 회귀와 macOS typing 통과 |
| AC008 | BK-WBS-07.08.07.15.09 | V; 9e68bbb4 같은 complete owner/pump·원본 error/all-member 마무리와 macOS typing 통과 |
| 결합 리뷰 | BK-WBS-07.08.07.15.10 | D; frozen2a7313c6 독립 정식 code/security PASS0, 이전 FAIL2 해결 |
| AC012 리뷰 수정 | BK-WBS-07.08.07.15.06 / .15.10 | 구현/개발 검증 완료; 원래 future packet 2개 보존·교체 전용 port·분류 후 재확인 회귀19개 포함 app3279/0/ignored8, 정식 code/security·독립 CLI QA PASS; 새 기능 분모 아님 |
| 결합 QA·AC009 문서 | BK-WBS-07.08.07.15.11 | V; 정식 리뷰 뒤 독립 CLI software·문서 PASS, 한 번 owning verify PENDING: 자동 attestation 없음; close 미완료 |
| 실제 Mac acceptance | BK-WBS-07.08.07.15.12 | E; owned Mac 없음·macos-host 누락, portable/typing과 별개 |

AC001의 사전 계약은 모든 행에 적용한다. AC011은 이번 WBS 분해/추적 문서
작업이며 별도 제품 기능 수를 추가하지 않는다. 아래는 **과거 retained-practice
작업의 당시 AC 연결 기록**이다. 현재 상태는 각 WBS 말단의 행을 우선한다.
`TASK__wbs-ongoing-work-breakdown`의 도구·문서는
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

- 2026-10-15 CoreAudio E75: callback/native ownership/backend/UI/solo/local 소프트웨어
  연결과 future 관측·history 승인 경합 수정을 마쳤다. frozen2a7313c6의 독립 정식
  code/security PASS0 뒤 CLI software QA와 문서 재검토 PASS를 확인해 .15.10을 D로
  갱신했다. 현재136/489(27.81%), 남음353·원래91/193이다. 한 번 owning verify가
  자동 receipt 없음으로 PENDING이어서 .15.11 V를 유지하고 실제 Mac .15.12 E도
  남긴다. 독립 direct 결과를 owning task close PASS로 바꾸지 않는다.

- 2026-10-14 사용자 요청에 따라 미완료 말단117개를 실제 산출물/소비자/검사 경계의
  383개로 분해했다. 기존 행 전체·완료106·조건부4·원래91/193을 보존했고 활성
  223→489, D106→135(27.61%)로 재기준화했다. 추가 D29는 기존 실제 근거의
  좁은 하위 범위를 드러낸 것이며 새 개발 실적이 아니다. inherited 선행·ID·부모
  상태·기능60개 coverage를 검사하고 독립 문서 근거 감사를 받았다.
  CoreAudio .15를12개로 연결해 현재 기반3개 완료와 native/consumer/review/QA/
  실기기 잔여 범위를 보이게 했다. 이후 현재 수치는 집계 명령이 계산한다.

- 2026-10-14 exclusive E68: native/app lib/tests Windows typing0와 실제 공통37·publication
  할당 각1, scoped Rust12 format/diff를 확인했다. native/consumer2개 W→V로 현재
  D105·V73·W20·N2·E15·P5·U3이다. 새 native9개는 미실행이며 완료105/223은 유지한다.

- 2026-10-14 exclusive E67: 실제 worker36·sizing22·외부99 및 Windows bin79 PASS,
  기본 장치 선택 RED2→GREEN2를 기록했다. portable1개 W→V, native1개 N→W로
  D105·V71·W22·N2·E15·P5·U3이다. native 작성/독립 gate는 남고 완료105/223은 유지한다.

- 2026-10-14 exclusive E66: 원래 .13을4개 하위로 나눠 활성220→223으로 확장하고
  source 기반 독립 계획 검토 뒤 software2개 W·native1개 N·실제 장치1개 E로 추적한다.
  새 완료는 없으며 D105·V70·W22·N3·E15·P5·U3,105/223(47.09%)다. 원래193/완료91과
  모든 native/다른 backend·형식·출시 의무는 유지한다.

- 2026-10-14 Source E65: actual 독립 DEEP review 다음 CLI QA PASS로 소프트웨어8개를
  V→D로 옮겼다. Source8D·4V·3N, 전체 D105·V70·W20·N3·E14·P5·U3이며
  105/220(47.73%)다. 원래91/193은 그대로다. 실제 Windows VM의 Render endpoint0·
  잠긴 console/현재 실행 환경 부재로 desktop BLOCKED_ENV; native4개와 full Goal은
  미완료다. hook 완료 증거도 없어 한 번의 verify는 PENDING이며 task close와 다르다.

- 2026-10-14 Source E64: 정식 리뷰3건 수정 후 actual RED2→GREEN과 새 검사·형
  검사를 확인해3개를 W→V로 복구했다. D97·V78·W20·N3·E14·P5·U3; 분모/완료
  수는 유지하며 독립 재검토와 실제 Windows는 남는다.

- 2026-10-14 Source E63: 실제 정식 리뷰 FAIL3 뒤 worker/native/Windows3개를
  V→W로 되돌렸다. D97·V75·W23·N3·E14·P5·U3; 완료 수/분모는 그대로다.

- 2026-10-14 Source E62: 실제 startup 수정 뒤 연결4·전체 app2773·Windows bin69와
  publication 할당 각1 검사를 통과해 controller/consumer/pump3개를 W→V로 옮겼다.
  Source12V·3N, 전체 D97·V78·W20·N3·E14·P5·U3이며 분모/완료 수는 유지한다.

- 2026-10-14 Source E61: 실제 연결에서 startup origin 결함을 확인해 controller를
  V→W로 되돌렸다. Source9V·3W·3N, 전체 D97·V75·W23·N3·E14·P5·U3이다.
  활성220·완료97과 원래91/193은 유지하며 실제 회귀 통과 전 복구하지 않는다.

- 2026-10-14 Source E60: UI를 W→V, Windows consumer/공통 pump를 N→W로
  갱신했다. Source10V·2W·3N이며 전체 D97·V76·W22·N3·E14·P5·U3,
  활성220·완료97과 원래91/193은 유지한다. 최종 검증은 병렬 레인 양쪽 이후다.

- 2026-10-14 Source E59: Owner/adapter2개를 W→V, typed UI를 N→W로 갱신했다.
  Source9V·1W·5N이며 전체 D97·V75·W21·N5·E14·P5·U3, 활성220·완료97과
  원래91/193은 유지한다. 실제 typed 오류 경합 RED/GREEN과 외부 할당 회귀를 기록했다.

- 2026-10-14 Source E58: native/controller2개를 W→V, 작성된 Owner/adapter2개를
  N→W로 갱신했다. Source7V·2W·6N이며 전체 D97·V73·W22·N6·E14·P5·U3,
  활성220·완료97과 원래91/193은 유지한다. 개발 검사와 Windows 타입 검사를
  독립 최종 검증·장치 실행 완료로 확장하지 않는다.

- 2026-10-14 Source 착수 분해: 기존07.08.07 하나를15개 실행 말단으로 나눴다.
  활성206→220, 구조 부모2→3이며 완료97과 원래91/193은 유지한다. E56의 공통
  native/pause/state/port 기초4개 W, 나머지11개 N으로 실제 개발/검증을 추적한다.

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
