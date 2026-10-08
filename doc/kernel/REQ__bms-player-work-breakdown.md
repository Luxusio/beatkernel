# BMS 플레이어 전체 작업 분해 구조(WBS)

기준일: 2026-10-08. 목표: **범용 BeatKernel 기반의 고성능 cross-platform BMS 플레이어**.
사용자가 요청한 전체 TODO/진행률의 기준 문서다. 현재 병렬 개발 묶음만으로
전체 목표를 대체하지 않는다. [원래 명세](../../plan.md),
[플레이어 요구사항](REQ__bms-player.md),
[전체 완료 조건](REQ__plan-acceptance-evidence.md),
[기능 ID 인벤토리](REQ__bms-player-progress.md)의 활성 요구를 유지한다.
원래 명세의 초기 비목표는 이후 사용자가 승인한 UI/browser/멀티플레이 요구를
제외하는 근거가 아니다. 검증 연기는 2026-10-06 해제됐다.

## 읽는 법과 집계

- **말단 체크박스만 센다.** 제목/부모/기능 BK 번호/테스트 개수는 분모에 더하지 않는다.
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

집계/ID/상태/선행/기능 coverage 확인:

```sh
python3 tools/wbs_status.py
python3 tools/wbs_status.py --json
python3 tools/wbs_status.py --ready
```

숫자는 위 명령이 본문에서 계산한다. 별도 수동 합계표를 동기화할 필요가 없다.
상태 줄의 `선행=-`는 이 문서에 추가로 명시한 실행 선행이 없다는 의미이며,
아래 실제 개발 의존성/공통 파일 장벽까지 없다는 의미가 아니다.

## 근거 인덱스

근거는 2026-10-08 현재 checkout에서 확인한 개발/QA 기록이다. 로그는 ignored
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
- [ ] **BK-WBS-02.09** 동일한 정수 판정 시간과 시각 보간 시간을 분리해 장시간 시각 drift를 검사한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=20시간/1주일 가상 시계와 실제 렌더 projection.
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
- [ ] **BK-WBS-04.12** SDVX axis·Arcaea dual-contact·Taiko repeated·GITADORA composite·osu pointer·VR pose 여섯 일반화 fixture가 각각 실제 입력/판정 결과를 재현한다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-04.04; 필요 근거/다음=plan §22의 여섯 조합을 이름별 suite/결과에 대응; 최소 네 종류 DoD만으로 여섯 조합을 완료 처리하지 않음.

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
- [ ] **BK-WBS-05.13** pitch 변화 허용 배속의 sample read-head/resampling과 명령 rate가 선언한 재생 속도에 맞는 PCM을 만든다. — 상태=V(구현됨·검증대기); 선행=BK-WBS-05.01; 필요 근거/다음=plan §11.4; Mixer rate fixture와 실제 Runtime/Transport 연동 범위를 분리하여 검사; device sample-rate 변경을 playback speed로 대신하지 않음.
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
- [ ] **BK-WBS-07.09** rate/buffer/channel/encoding 변경을 실제 설정 요청·capability·applied 결과까지 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E19 target rate/buffer/period/matrix UI bridge 검증; 실제 launcher/encoding 제어·pending PCM 해석 보존은 계속 필요.
- [ ] **BK-WBS-07.10** startup/pause/resume/finite end ACK가 lookahead/held-release가 아니라 native crossing을 기다린다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E16 공개 audio pump의 pause/resume/held 순서 검증; 실제 launcher owner→worker→native 관측 E2E 및 finite end는 계속 필요.
- [ ] **BK-WBS-07.11** WASAPI↔ASIO 등 cross-backend 교체의 현재 소스 범위와 missing seam을 audit한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=native source/phase/history/latency/cleanup 교차 검사.
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
- [ ] **BK-WBS-08.07** key/scratch/LN-end별 versioned timing preset을 기존 ClassifiedWindow에 연결한다. — 상태=N(미구현); 선행=-; 필요 근거/다음=검증된 opt-in dialect·경계 ±1ns; 새 기본값 추정 금지.
- [ ] **BK-WBS-08.08** 기본 timing dialect와 역사적 LR2/nanasi 호환 범위를 확정한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=원문 spec만으로 millisecond 표를 추정하지 않음.
- [ ] **BK-WBS-08.09** 동적 EXRANK/A0 의미와 필요한 timed policy를 primary source로 확인한다. — 상태=U(현황감사필요); 선행=-; 필요 근거/다음=서로 다른 engine 동작을 동일 표준으로 합치지 않음.
- [x] **BK-WBS-08.10** fresh unmatched/repeat/refused 입력을 구별하는 authoritative empty-POOR disposition을 만든다. — 상태=D(검증완료); 선행=-; 근거=E32,REQ__input-disposition; penalty/dialect 선택은08.11에 유지.
- [ ] **BK-WBS-08.11** empty-POOR의 후보/반복/게이지/combo/capture 의미를 선택 dialect에 고정한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=preroll 정책과 구별; replay/network identity에 포함.
- [x] **BK-WBS-08.12** WAV/FLAC/Vorbis/MP3가 실제 bounded preparation과 Mixer 출력까지 연결된다. — 상태=D(검증완료); 선행=-; 근거=E1; 포맷별 declared trim/limits.
- [x] **BK-WBS-08.13** same-format Ogg chaining의 BOS/EOS/serial/독립 trim/aggregate limit를 검증한다. — 상태=D(검증완료); 선행=-; 근거=E1; malformed·mixed format 전체 거절.
- [ ] **BK-WBS-08.14** 전 포맷의 잘린 파일·큰 metadata·late decode 실패·rate/channel 정책을 확장 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=순수/실제 파일 corpus와 allocation/byte budget.
- [ ] **BK-WBS-08.15** static BGA BMP/PNG/JPEG·layer/poor/opacity/crop/canvas·파일명 호환을 실제 화면에서 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=기존 순수 fixture + 실제 native/browser rendering.
- [ ] **BK-WBS-08.16** EXBMP RGB/color-key/근사 matching의 정확한 호환 규칙을 확정한다. — 상태=P(정책선택대기); 선행=-; 필요 근거/다음=원본 engine 문서/asset 사례·허용 오차.
- [ ] **BK-WBS-08.17** video decode·timestamp/reorder/frame budget·off-thread 공급을 구현한다. — 상태=N(미구현); 선행=-; 필요 근거/다음=audio master clock 기반; native/browser decoder port.
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
- [ ] **BK-WBS-09.11** gapless loop/scrub에서 같은 출력 timeline을 유지하며 reopen gap을 제거한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=source/target phase·BGM/키음·finite 경계 E2E.
- [ ] **BK-WBS-09.12** 장시간/반복 구간 재시작의 실제 키음/BGM/노트 동기화를 측정한다. — 상태=E(환경·장비대기); 선행=-; 필요 근거/다음=물리 출력/loopback; 논리 hash만으로 완료하지 않음.
- [ ] **BK-WBS-09.13** profile/replay/archive 저장의 interruption·동시 publish·symlink·capacity 오류를 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 파일 I/O·잘린 파일·재실행.

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
- [ ] **BK-WBS-10.16** 개별 component fractional transform/opacity/easing을 paint/hit/clip에 공통 적용한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E17/E18 browser Selection/Display, E24 native Selection/Display 소유자, E25 실제 native software Vulkan 이동/역 hit; 나머지 화면 연동·하드웨어 및 독립 QA 필요.
- [ ] **BK-WBS-10.17** component animation scheduler의 frame budget·취소·suspend/resume/dispose를 구현한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=E17 64-slot/수명/cache·worker scheduling, E18 browser 51프레임/dispose, E24 native 수명 fixture, E25 native 22프레임/cache/dispose; 실제 성능/frame budget 및 독립 QA 필요.
- [ ] **BK-WBS-10.18** font fallback/Unicode/IME/clipboard/selection이 실제 native/browser 입력 방법에서 동작한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=순수 editor fixture + 실제 API/글꼴.
- [ ] **BK-WBS-10.19** 0-size/resize/DPI/focus loss/renderer loss/close가 owner cleanup과 input geometry를 보존한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 winit/GPU/browser lifecycle.
- [ ] **BK-WBS-10.20** Selection→설정→로딩→플레이→pause/output→기록/결과→Back/재시작 전 흐름을 GUI로 검증한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 shader/클릭/키/수명·독립 GUI QA.

### 11. 브라우저 WASM·Worker·오디오·입력

기능 연결: BK-040, BK-041, BK-042, BK-045, BK-047, BK-050, BK-051, BK-052, BK-053. 요구: REQ__bms-browser / browser-retained-menus.

- [ ] **BK-WBS-11.01** 현재 source의 실제 WASM library/AudioWorklet/render package를 빌드한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=pinned wasm-bindgen과 native-free dependency 검사.
- [ ] **BK-WBS-11.02** Gameplay Worker가 준비·판정·capture·network owner를 담당한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=현재 실제 bridge·stalled renderer 관찰.
- [ ] **BK-WBS-11.03** Renderer Worker/OffscreenCanvas가 GPU와 local retained views를 소유한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=실제 current WASM renderer·Scene/메뉴 수명.
- [ ] **BK-WBS-11.04** 모든 메뉴의 business owner·상태 packet·semantic action을 실제 Renderer view에 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=Selection/Settings/Practice/Display/Records/Players/Devices/Results.
- [ ] **BK-WBS-11.05** 실제 Records preview·own/other opponent·history를 read-only packet으로 연결한다. — 상태=W(구현·연동중); 선행=-; 필요 근거/다음=AC024 모델을 사용한 actual browser navigation.
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
- [ ] **BK-WBS-12.04** 브라우저가 actual BrowserRoomOwner/WASM identity로 WebTransport room을 재생한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=trusted HTTPS profile; native/browser 상호운용.
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

- [x] **BK-WBS-13.01** seeded property/corpus가 시간·codec·compiler·replay 기준 결과와 일치한다. — 상태=D(검증완료); 선행=-; 근거=E0; 장기 fuzz 완료와 구별.
- [ ] **BK-WBS-13.02** 모든 layer의 pure test port와 실제 adapter contract test를 coverage map으로 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=UI/business/native IO 의존성·경계 표.
- [ ] **BK-WBS-13.03** parser/compiler/input/replay/room codec에 지속 fuzz·corpus/minimization을 구축한다. — 상태=N(미구현); 선행=-; 필요 근거/다음=시간/seed/artifact 재현 가능한 campaign.
- [ ] **BK-WBS-13.04** 수집·queue·clock·output·network·UI에 deterministic fault/chaos schedule을 구축한다. — 상태=N(미구현); 선행=-; 필요 근거/다음=allocation/short write/spawn/panic/stale/drop 동시 경계.
- [ ] **BK-WBS-13.05** 대규모 노트·64-member·오랜 플레이·반복 seek/rebind의 soak/stress를 실행한다. — 상태=W(구현·연동중); 선행=-; 근거/다음=E33,REQ__dense-chart-stress의 actual judge/replay/projector workload 검증 완료; 실제 CPU/memory/queue/owner leak·20h/1week 전체 가상 timeline·다인/rebind/soak는 계속 필요.
- [ ] **BK-WBS-13.06** 전체 RT callback과 판정 hot path alloc/lock/I/O/error 경로를 계측한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=일반/오류/포화·모든 backend; scoped 테스트만 확대하지 않음.
- [ ] **BK-WBS-13.07** p50/p95/p99/max·drop/xrun·scheduler jitter 계측을 재현 workload에 연결한다. — 상태=V(구현됨·검증대기); 선행=-; 필요 근거/다음=계측 위치/provenance/용량·관측 불가 표시.
- [ ] **BK-WBS-13.08** 성능 기준 workload·CPU/RAM/GPU/배포 artifact/실행 절차를 고정한다. — 상태=W(구현·연동중); 선행=-; 근거/다음=E31 fixed software matrix/report 검증 완료; 대형 chart·dense notes·UI stall·다인·GPU·장시간 비교는 계속 필요.
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

## 현재 병렬 task와 연결

현재 task `TASK__parallel-player-requirements`의 **24개 AC / 17개 개발 milestone**은
아래 WBS의 일부다. 전체의 분모 또는 별도 추가 24개 작업으로 합산하지 않는다.

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
