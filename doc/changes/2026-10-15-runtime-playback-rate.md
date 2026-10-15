# 실행 중 배속 전환의 PCM·Transport·기록 기반

구현 중인 WBS05.13.06을 위해 PCM과 Transport가 공유할 `PositiveRatePhase`를
추가했다. 실제 cut에서 분수 ns를 보존하며, signed preroll을 한 번만 truncate하고,
정규화한 같은 배속 요청은 anchor를 바꾸지 않는다. cue 역산은 기존 음수 target
의미를 유지하면서 상수 개수 정수 연산으로 처리한다. 기록용 accessor는 정수
표시값과 구분한 정확한 원본 phase를 제공한다. 계약과 설계는
[REQ](../kernel/REQ__runtime-playback-rate.md)와
[ADR](../kernel/ADR__runtime-rate-phase-and-receipts.md)에 있다.

첫 기반 검증은 새 독립 integration19개와 기존 live_transport_speed16개,
output_phase16개, practice_speed20개로 총71개가 통과했다. 새 suite는 literal
44.1/48kHz cut, 독립 fixed-grid 변경 이력 oracle, 큰 정수/분모·역산·오류와
calibrated allocation/reallocation/deallocation0을 확인했다. 앞선 독립 test-author의
16개 실행은 subset이므로 별도 신규 통과 수로 합산하지 않는다.

후속 구현은 역할별로 커밋했다.

- `28306f66`: 공유 exact phase와 기록용 분수 accessor.
- `15403ce1`: 실제 rate cut·pause/resume 이력과 같은 logical ns의 정확한 E 선택.
- `5d2fd992`: 원본 operation을 유지하는 가변 source rate schedule·capture byte
  accounting·전체 파일 제한과 fixed competition의 명시적인 거절.
- `524a5d30`: 실제 첫 active PCM frame의 all-head/cursor 결정, typed event와
  projection, 현재 배속 scrub/loop, 원래 배속의 compound restart.
- `da576ffc`: 단일 pending slot의 typed rate/region/original-restart 제어와
  실제 Applied/Noop/Refused 상태·revision 검증, legacy host 기본 거절.
- `144bf04c`: retained Worklet Program·portable 배속 준비·실제 PCM 이벤트와
  pause/history pin·strict numeric ABI 및 BrowserAudio export.
- `faed4bec`: 초기 judge만 선택하고 원본 PCM/binding/BGM을 이동 보존하는
  browser retained 준비와 최초 advance 이전 opt-in.
- `7247d64c`: 실제 native proof와 공통 controller-free rate business,
  모든 member의 Transport/header/capture 반영과 현재 배속 구간·원래 설정 F5.
- `6cad2ac3`: Play lifecycle에 붙는 retained 선언형 RateView와 typed host 전달,
  키보드·IME·clipboard·실제 결과 기반 배속 제어.

현재 core 전체는82개 suite의 **816/0**, app의 기록·리플레이 관련7개 suite는
**81/0**이다. app에는 새 identity9개·capture10개와 기존 회귀62개가 포함된다.
앞선 phase71개·Transport112개·PCM/할당34개는 전체 core에 포함된 subset이며
별도 신규 통과 수로 더하지 않는다. 실제 명령 원본은 ignored 작업 폴더
`target/wf/runtime-rate-14043/core-complete-with-race.log`,
`capture-identity-after-file-cap.log`, `pcm-after-owner-and-burst.log`에 있다.

새 PCM23개와 할당11개는 분수 live head, 마지막 voice의 실제 i128 분모 용량
실패, pause pending, converted lookahead/held output, 거절 뒤 PCM·credit 보존,
현재 배속에서의 구간 표현 범위, 다음 반복의 voice·빈 cue 작업 용량을 검증한다.
추가 private fixture는 기존 production preflight/per-frame seam으로 callback
snapshot 뒤 publication과 다음 실제 render의 StaleGeneration 거절을 결정적으로
재현한다. 공개 test hook이나 sleep을 추가하지 않았다. live portable bind/token
drop과 실제 render·event pop의 calibrated alloc/realloc/dealloc은0이다.

초기 실행에서 발견한 다른 owner token 수용과 파일 전체 제한 누락을 수정했다.
preroll carry BGM을 무음으로 기대한 값과 Ended의 generation 증가를 놓친 값은
기존 REQ·공개 회귀의 명시적인 계약으로 바로잡았다. 실제 실패 원본과 이후
통과 결과는 구분하며 기대값을 구현에 맞춰 약화하지 않았다.

최신 strict core Clippy는 수정하지 않은 source_playback_history.rs::preflight의
기존 too_many_arguments1건으로 실패한다. 앞선 practice.rs의 인자 수 경고는
private ProgramTiming으로 정리됐고 경고 억제는 추가하지 않았다. scoped Rust
format과 diff 검사는 통과했다. app은 기존 파일별2021/2024 style을 보존하며,
multiplayer.rs의 기존 format 차이는 두 style 모두 변경 전후 동일함을 확인했다.

Worklet public integration24개와 library Worklet/output regression24개,
공통 typed mailbox30개가 각각 통과했다. Library24개에는 새 retained report2개와
legacy owner1개가 포함되며 별도 신규 개수로 중복 합산하지 않는다. 첫 integration
21/2는 성공 literal의 context 시작+상대 frame overflow를 유효한 wide 값으로
바로잡고 기존 불가능한 조합의 명시 거절 검사를 추가해 해결했다. 첫 mailbox
28/2는 동일 revision에서 StaleRevision이라고 표시한 두 입력을 실제 일반 용량
거절로 수정하고 거짓 stale 사유 거절 검사를 보존했다. Library 첫23/1은 실제
누적 rendered_frames 불일치를 놓친 retained decoder 결함이었으며, fresh Mixer의
누적 physical end 일치 검사를 추가해24/0이 됐다. 기존 plain/finite decoder는
변경하지 않았다. 실패 로그와 corrected 로그는 같은 작업 폴더에 따로 보존한다.

이는 실행 중 배속 전환 전체의 완료가 아니다. 현재 AC001–005·007·012·013·015의
소프트웨어 기반 검증을 마쳤다. AC012 오디오 WASM의 첫 check는 cold sample_limit accessor 누락으로
실패했고 누락을 추가한 뒤 browser-audio lib check가 통과했다. scoped format은
기존 파일별2021/2024 style을 유지해 통과했다.
공통 native pump·실제 시작 경로와 browser 수직 연결, 실제
WASM/화면 동작, 전체 독립 code/security review와 필수 QA는 남아 있다.
WBS05.13.06은 W, 완료 수는136/489다. 기존 Chromium strict network와 실제 장치
acceptance의 환경 한계도 유지한다. Native IO06은 입력 항목도 포함하므로
그 전체0/32를 순수 오디오 진행률로 합산하지 않는다.

실제 native launcher6곳과 공통 audio/startup/policy가 여전히 fixed constructor를
사용하는 연결 누락도 확인했다. AC014를 같은 기능의 후속 조립 단위로 명시하고
AC010 통합 검사의 선행으로 추가했다. 새로운 제품 요구나 WBS 완료 수 증가는
아니다. 이어서 AC005 owner·AC007 UI·AC015 원본 browser 준비의 구현/독립 테스트
후보를 작성했다. 모든 writer가 멈춘 뒤 합친 browser WASM lib check가 통과했다.
공통 owner10/0, SessionLaunch7/0, 기존 lineage5/0, capture12/0,
retained 준비5/0, timing policy7/0, native presentation97/0 및 rate library108/0을
확인했다. presentation97과 rate108에는 같은 새 native proof4개가 포함되므로
합산하지 않는다. timing policy7개 중 retained 신규3개와 public 준비5개가 AC015의
신규8개다. public native controls11/0과 retained RateView 신규8개(rate108 내부)도
통과했다. 실제 desktop main bin 제어는 첫13/1의 marker를 수정한 뒤14/0,
정확한 IME module 검사는3/0이다. 두 잘못된 lib/bin IME·desktop 필터의0개
실행은 검증 근거로 계산하지 않았다. 실제 명령 결과는
ui-desktop-bin-controls-marker-corrected.log와 ui-desktop-bin-ime-correct-filter.log에 있다.

AC005 후보 검토에서 실제 적용 후 prepare 실패/폐기 시 기록 공간 예약이 풀리는
공백도 확인했다. pending 원본 token을 유지하고 cold clone을 준비에 사용하도록
수정했다. clone은 같은 예약 수명을 공유하며 commit/명시 취소는 다른 clone을
무효화한다. 이 경로의 late report·동일 실제 cut retry·stale clone에 대한 신규
회귀3개를 추가해 실제 통과했다. callback이나 입력마다 header를 clone하는 변경은
아니다. 두 fixture의32f32에64byte asset cap을 준 오류는128byte로 바로잡았고
PCM 기대값은 유지했다.

이후 owner constructor가 raw Mixer domain과 normalized replay domain의 equality를
잘못 요구하는 실제 결함도 확인했다. 원래 epoch 계약에 맞게 header 논리 domain의
member 일관성과 실제 Transport/native proof의 logical output을 검사하도록 수정해
owner10/0을 확인했다. RAW2/LOGICAL3은 그대로 검증하며 실제 입력이나 시계를
재작성하지 않았다. desktop F5 fixture의 Refused marker를 Requested로 표기한
입력도 ControlRejectedRevision으로 바로잡고 거짓 성공 marker 거절을 추가했다.

다음 병렬 단위는 AC006 공통 native pump·AC008 browser Step/WASM owner·AC014
6개 실제 native 시작 경로다. 각 구현과 독립 테스트 작성자를 분리하고 빌드는
관련 writer가 모두 멈춘 뒤 실행한다. 이 연결 중 발견한 기존 group의 초기
anchor-rate 비교와 F5 원래 배속 복구 충돌은 공통 RuntimeGroup/SoloRuntime에서
해결하며, browser fresh 경계의 old capture 회수는 bounded 단일 cohort slot과
별도 export로 유지한다. 이 후속 변경은 아직 검증 완료로 표시하지 않는다.

2026-10-15 후속 연동: 공통 native solo/cohort가 실제 Source rate cut과 첫 Target
projection을 분리하고, raw PCM 경계가 아직 투영되지 않았으면 cut 이상 입력과
frontier만 보류한다. Pause/Resume/adoption은 계속 처리한다. 같은 attempt의
Transport/header/capture와 2·4인 capture 활성·비활성 입력 순서를 회귀로 확인했다.
6곳 실제 native 시작 경로도 공통 runtime Program/owner를 조립한다. 초기 seed와
선택 judgment class의 strict RecordPreview 검증은 복구했고, 명시적인 가변 기록
history 읽기는 유지했다.

현재 합친 host 검사에서 앱 lib3356 PASS/실패0/ignored9와 관련 integration99
PASS/실패0을 확인했다. 이는 겹치는 기존·신규 회귀를 포함한 실행 수이며 신규
기능 수가 아니다. 원본은 native-raw-barrier-fixture-calls-14108.log다. Windows와
macOS lib/tests/bins C/archive-stub 타입 검사도 exit0이며 실제 SDK/driver 실행은
아니다. Browser Step/export 연결은 실제 UI/audio release WASM과 matching cached
bindgen 생성까지 통과했다(browser-release-wasm-14107.log,
browser-audio-release-wasm-14107.log). 실제 브라우저 acceptance는 별도다.

브라우저 JS는 Worklet·포트/host·Worker/Window 세 쌍으로 구현과 독립 테스트를
나눴다. 첫 focused 실행은79개 중38 PASS/41 FAIL이며 raw 로그는
js-focused-first-14109.log에 있다. Worklet 공통106은 계수용 VM BigInt spy의 표준
asIntN 누락으로 확인돼 fixture를 교정했다. 이후 Worklet·포트의 새/기존 Node
231개가 모두 통과했다(js-worklet-ports-corrected-14109.log). Worker/Window의
타이머·장치 binding·DOM 설정 오류와 실제 구현 결함은 구분하며 새 실행으로
확인한다. 이는 개발 회귀 단계이며 독립 전체 review/QA PASS나 WBS D 승격을
뜻하지 않는다. WBS05.13.06은 W, 전체136/489·오디오12/27을 유지한다.

후속 수정에서 남은 raw timestamp 사례는 실제 serial pump wake 결함이었다.
poll ACK 대기 중 온 retained 요청을 보관했으나 기존 commandBatch가 비면 drain이
종료해 다음 외부 tick까지 전송하지 않았다. 같은 drain이 pending retained 요청을
다시 확인하도록 최소 수정했고, 해당 literal timestamp/실제 순서 oracle은 유지했다.
별도 검토에서 찾은 malformed current File receipt는 원본 File을 보존하고 custody
승인·Worker 해제를 보류하도록 수정했다. 완전한 wire의 current+retired 개수3/4
검사와 malformed 응답→동일 File 유효 재시도 회귀를 보존했다.

수정 후 신규 Worker/Window33개와 기존 관련504개가 모두 통과했다.
js-worker-window-wake-fixed-14109.log 및 js-worker-window-legacy-final-14109.log가
원본이며, 앞선231개와 모듈별 실행을 구분한다. 실제 PCM/기록/replay 추가3개는
첫 실행2 PASS/1 FAIL로 converter의 sample literal 원인을 독립 조사 중이다.
실제 브라우저 runner는 별도 작성 중이며 formal review/QA는 아직 시작하지 않았다.

검증한 source 후보는 ecc744a5(native), c0e398ed(browser Step/export),
8bd1b2ca(Worklet/ports), dd567eeb(Worker/Window/capture custody)로 나눠 커밋했다.
이들 커밋은 최종 전체 acceptance를 뜻하지 않으며 push하지 않았다.

추가 end-to-end3개도 최종 통과했다
(pcm-step-replay-end-to-end-prefill-fixed-14109.log). 실제 Step발행 키음과 BGM의
연속 PCM, capture decode/reconstruction 및 별도 replay 출력 배속,44.1/48kHz
odd cut·preroll·지연 입력, 양방향 실제 ConvertedMixer의 분수·최초 crossing을
검사한다. 초기 converter fixture의 legacy scope0과 reverse prefill 부족을 실제
생산 generation/선형 lookahead에 맞췄으며 모든 PCM·분수·cut 기대값을 유지했다.
Converter generated facts와 실제 native/물리 presentation 증거는 구분한다.
