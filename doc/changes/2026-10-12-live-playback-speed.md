# 고정 실시간 플레이 배속 — 구현 진행 중

실시간 플레이가 원곡 좌표를 유지하면서 양의 유리수 배속을 선택하도록
공통 Transport와 출력 스케줄링을 확장하고 있다. 입력 수집 시계와 실제
키음 출력 시각은 배속을 곱하지 않는다. 출력 preroll과 일시정지 시간은
출력 시간 단위로 유지하며, 원곡 위치는
`start + trunc((active_elapsed - preroll) * numerator / denominator)`로 계산한다.

## 현재 개발 검증

- 새 Transport 계약 테스트 16개와 기존 시간·Transport 테스트 10개가 통과했다.
  홀수 나노초 preroll, 실제 Runtime 판정·입력 provenance·키음 출력 시각,
  일시정지 후 분수 위상, 역사적 조회, 원자적 거절과 정수 범위를 검사한다.
  원본 로그: `target/wf/live-speed-development-12d03/transport-2.log`.
- 브라우저 공통 모델의 새 테스트 14개와 기존 테스트 44개가 통과했다.
  기존 replay 모델 함수를 직접 export alias로 재사용한다.
  `node-model-result.json`은 coordinator의 실행 요약이며 원본 TAP이나
  독립 QA 증거를 대신하지 않는다.
- 공통 `LiveSpeedSetup` 계약 테스트 14개가 통과했다. 네 배속의 원곡 위치와
  BGM 역변환, 음수 시작점, 출력 도메인 보존, 0 프레임 종료점, 실제 frame grid
  올림과 전체 signed span의 마지막 범위 검사를 확인했다.
  원본 로그: `target/wf/live-speed-development-12d03/live-setup-1.log`.
- native cold 오디오 계약 테스트 11개가 통과했다. 실제 Mixer PCM, 배경음이
  없는 경우의 origin SetRate, 원시 입력·Stop 시각, finite endpoint와 큐 credit,
  시작 gate 및 NORMAL 호환성을 검사했다.
  원본 로그: `target/wf/live-speed-development-12d03/native-audio-1.log`.
- 새 pause 계약 14개, 새 Settings/profile 계약 14개와 기존 app 단위 테스트
  1960개가 통과했다. 같은 실행에서 공통 설정 14개와 오디오 11개도 다시
  통과했다. 기존 app의 ignored 4개는 실행하지 않았으며, 직접 장치 검증을
  대신하지 않는다. 원본 로그:
  `target/wf/live-speed-development-12d03/shared-regressions-1.log`.
  source/converted target/interval pause의 실제 acknowledgement, 분수 위상,
  원자적 거절, 실제 AudioAuthority token, 기존 Settings 행 순서와 profile
  v1/v2 기본 바이트 호환성을 확인했다.
- 기록 배속 계약 테스트 13개가 통과했다. NORMAL 바이트 보존, 잘못된
  envelope 거절과 전체 header 예산, 실제 Runtime 입력·판정·gauge·hash·
  provenance의 기록/복원, 별도 replay 출력 배속 및 native catalog 조건
  비교를 검사했다. 원본 로그: `live-recording-4.log` (같은 증거 폴더).
  앞선 실행의 test fixture 정수 타입, WAV 정의와 두 탭 사이 release 오류는
  각각 고쳤으며 `live-recording-1.log`부터 `live-recording-3.log`에 보존했다.
  파서나 판정 검증을 완화하지 않았다. 최종 source의 전체 app
  회귀와 실제 플레이 소비자·독립 QA는 아직 남아 있다.
- 공통 solo/local 플레이의 새 테스트 14개와 native policy의 public 호환
  테스트 1개가 `live-owners-3.log`에서 통과했다. 실제 네 배속의 Mixer PCM,
  키음·mine·gauge, 원곡 입력/기록 복원, activation, zero/quantized finite end,
  실제 출력 표식·presentation·credit, local 2/4/64 member 종료를 검사한다.
  64 member는 순수 component fixture이며 64 물리 장치 검증이 아니다.
  유한 종료에서 실제 SetRate credit을 회수하지 못하던 결함은 검증된
  RenderReport cursor로 반납하도록 수정했다. 최초 실패는 `step-live-1.log`에
  보존했다. 새 native private admission 12개를 포함한 app 단위 테스트
  1972개와 확장된 기록 테스트 15개는 `live-owners-2.log`에서 통과했다.
  해당 실행의 public fixture WAV 정의 오류는 이후 수정했고, Step의 마지막
  credit 수정 이후 전체 app 단위 테스트 재실행은 최종 회귀 단계에 남아 있다.
- 최초 전체 core 검사 `core-full-1.log`는 컴파일 중 240초 제한으로 종료됐다
  (exit 124). 제한을 600초로 늘린 후 같은 source를 재검사했고
  `core-full-2.log`에서 62개 suite의 531개 테스트가 모두 통과했다
  (exit 0, failed 0, ignored 0, doctest 포함).
- 실제 native/browser 소비자 연결과 독립
  review·QA는 아직 진행 중이다. 새 cold API의 통과는 아직 실제 앱에서
  배속을 선택하고 플레이하는 전체 경로의 완료를 의미하지 않는다.
- 네이티브 solo/cohort 소비자 후보는 컴파일됐다. 최초 solo 13개는
  fixture `PcmLimits`의 asset/total byte 예산이 역전돼 시작 전에 실패했다.
  cohort는 3개 통과·7개 실패했으며, 실제 동일 조건의 shared competition도
  기존 nondefault 거절 분기에 걸리는 구현 결함을 드러냈다. 원본 로그는
  `native-solo-1.log`, `native-cohort-1.log`다. 두 수정과 재실행은 진행 중이며
  이 결과를 native 통합 PASS로 표시하지 않는다.
- 새 browser main/audio WASM 빌드와 bindings 생성 후 실제 분리된 두 WASM을
  사용하는 16개 export 테스트가 `browser-export-3.log`에서 통과했다.
  solo/local 네 배속의 실제 PCM·ACK·판정·기록 및 별도로 선택한 replay 배속,
  legacy typed input, zero/quantized 종료 권한과 취소/재시도를 검사했다.
  이전 fixture의 keyboard control 종류, 최초 presentation anchor 및 1ns의
  출력 양자화 가정 오류는 `browser-export-1.log`/`browser-export-2.log`에
  남겼으며 판정·종료 oracle을 완화하지 않았다. 이 결과는 실제 브라우저
  UI→Worker→Worklet 검증이나 독립 qa-browser PASS를 대신하지 않는다.
  바인딩 구현과 테스트 커밋은 `ecb9130`이다.
- `native-owners-3.log`에서 cohort 통합 12개가 통과했다. matching shared
  competition, 2/4/64 member, 실제 변환 출력 및 ASIO interval pause를 포함한다.
  초기 origin SetRate는 실제 BgmFeeder/RuntimeGroup credit 포트로 준비했다.
  solo는 15개 중 13개 통과·2개 실패이며, 2x 유한 종료가 아직 완료되지 않는
  경로를 조사 중이다. 이 실패를 제외하거나 native 완료로 표시하지 않는다.
- 원인 확인 실행 `native-solo-diagnostic-1.log`는 획득·closed prefix가 같고
  pending 입력이 0임을 확인했다. source Mixer 종료 후 도착한 유효 입력의
  키음이 producer 큐에 들어갔지만 종료한 Mixer가 소비하지 못하는 문제다.
  완료 ACK 검사는 유지하며, 실제 finite 출력 표식 이후 판정·기록과 새
  게임플레이 소리 발행을 분리하는 공통 Runtime 경계를 추가 중이다.
- Worker의 불변 live 배속 선택·canonical metadata·실제 bindings 조건 대조는
  `worker-2.log`에서 새 13개와 기존 155개, 총 168개 테스트가 통과했다.
  `worker-1.log`의 테스트 input mode·RPC 오류 계약·File 크기 불일치와 기존
  exact 응답 literal은 수정했으며, 기존 판정·소유권·ACK oracle을 유지했다.
  Worker 테스트의 generated-owner/browser 포트는 mock이고 실제 WASM 증거는
  별도의 위 16개 export 테스트다. UI와 실제 qa-browser는 아직 남아 있다.

- UI의 불변 live 배속 draft와 response identity 검사는 `host-2.log`에서
  새 테스트와 기존 host 회귀를 합쳐 309개 모두 통과했다. 이 테스트는
  실제 화면·Worklet 실행을 대신하지 않는다. UI와 fixture 커밋은 `d369067`이다.
- 공통 Runtime의 finite gameplay audio seal은 `ac4bf97`에 기록했다.
  `finite-audio-core-5.log`의 6개와 `finite-audio-app-1.log`의 3개 테스트가
  통과했다. 출력 종료 뒤 유효 입력의 원곡 판정·기록·mine/gauge는 유지하고
  소비 불가능한 새 gameplay 소리만 막으며, 기존 명령 credit을 지우지 않는다.
  수정 후 `core-full-3.log`에서 63개 suite, 총 537개 테스트가 통과했다
  (failed 0, ignored 0, doctest 포함). 네이티브와 Step의 seal 연결은
  추가됐으나 현재 재검증 중이다. 실제 callback 경합까지 해결됐다는 증거는 아니다.
- `seal-owners-3.log`에서 native solo 15개와 cohort 14개가 모두 통과했다.
  구현·테스트는 `bcbea26`에 기록했다. Step은 17개 중 15개 통과·2개 실패였고,
  새 fixture가 입력 frontier Advance와 별도의 terminal Advance 중 하나를
  기대 기록 목록에서 빠뜨린 문제를 확인했다. 정확한 6개 연산 순서·provenance와
  반복 완료 조회의 무추가 기록 조건으로 수정 후 재검사한다.
- 소스 조사로 finite 종료 직전 scheduling report 조회와 실제 command publication
  사이의 callback 경합이 아직 닫히지 않았음을 확인했다. 현재 순차 테스트 통과는
  이 경합의 완료 보증이 아니다. 실제 queue/Mixer interleaving 재현과 명령의
  정직한 종료 정산 설계·검증이 남아 있으며 ACK/applied를 가산해 숨기지 않는다.
- 브라우저는 유효 분수 MAX/1도 100ms preroll에서 초기 곡 시각이 범위를
  벗어나는 경우 오디오 생성 전에 거절하도록 공통 cold-phase 검사를 추가했다.
  `cold-phase-1.log`는 502개 중 501개 통과·1개 실패였다. 새 무제한 host
  fixture에서 함수 기본 endFrame이 유한 값으로 남던 문제를 명시적 undefined로
  고쳤으며 재검사 중이다. 실제 브라우저 QA 완료를 뜻하지 않는다.
- `cold-phase-2.log`에서 host·Worker 기존 회귀와 새 cold phase 테스트를
  합친 502개가 모두 통과했다. 수정과 테스트 커밋은 `f150f75`다.
  Step 재검사 `seal-step-4.log`는 16개 통과·1개 실패이며, 새 local fixture가
  명시적인 fail/export 뒤에 commands를 조회하던 순서 오류를 확인했다.
  command 검사는 fail 이전으로 이동하고 실제 Mixer counter·pending 조건을
  유지했다. 최종 재검사는 공통 native 준비 함수의 소스 동결 이후 수행한다.
- `terminal-publication-probe-2.log`는 실제 최신 core rlib와 Runtime·Mixer로
  종료 경합을 재현했다. 10frame 보고서 조회 뒤 실제 11frame endpoint를
  통과하고, 원곡 6ms의 유효 입력이 이전 raw 10ms에 소리를 발행하는 순서다.
  Runtime admitted=1이지만 실제 무음 callback 3회 뒤에도 Mixer consumed=0,
  applied=0이다. raw 시각이 endpoint보다 앞이므로 단순 timestamp 필터도
  해결이 아니다. 진단 성공은 결함 재현 증거이며 수정 PASS가 아니다.

- `preparation-owners-3.log`에서 common Step 17개, native solo 15개,
  native cohort 14개, 새 native 준비 9개, 총 55개가 통과했다.
  Step 연결·정확한 capture oracle 커밋은 `21615e5`, native 준비·얇은
  bridge 연결과 실제 saved-record 조건 테스트 커밋은 `76fb546`이다.
  새 배속의 saved 비교는 실제 public capture 바이트를 사용하며 동일 조건을
  받고 배속·finite section 차이는 거절한다. NORMAL의 역사적인 비교 바이트는
  유지한다. 앞선 두 compile 실패(없는 ReplaySpeed 상수와 fixture ID 타입)는
  `preparation-owners-1.log`/`preparation-owners-2.log`에 보존했다.
  현재 Linux/Windows/macOS 실행 파일의 소비 경로는 구현 중이며,
  최신 WASM 재생성·실제 브라우저 실행·독립 review/QA는 아직 남아 있다.

## 유한 종료 명령 정산 개발 증거

`terminal-core-1.log`의 실제 명령은 exit0으로 종료했다. 기존 Mixer 35개,
gameplay seal 6개, 새 terminal settlement 10개로 총 51개 테스트가 통과했다.
이전 보고서 조회 뒤 마지막 callback이 끝나고 원본 Runtime 입력이 소리를
발행하는 경합을 실제 consumer로 검사한다. 후속 nonempty silent callback이
명령을 실제 소비하고 `commands_expired`로 기록하며, `commands_applied`를
증가시키지 않는다. 원본 판정·provenance·raw audio timestamp와 종료 상태는
유지된다. pending과 queue는 하나의 callback 예산을 공유한다.

새 공개 counter의 전체 literal과 native/browser report 전달, 실제 완료 조건은
별도 연결 작업 중이다. 이 집중 테스트는 브라우저 실행이나 독립 QA 증거를
대체하지 않는다. 이전 경합 재현 로그는 수정 전 결함 증거로 유지한다.

보고서 연결 후보에 대한 `terminal-wire-node-2.log`는 603개, 원래 배속
선택·cold phase 회귀의 `terminal-cold-phase-node-1.log`는 70개가 실제
exit0으로 통과했다. 기존 scalar 인덱스를 유지한 새 보고서와 구버전 provider,
길이·backing 오류 거절을 검사한다. 첫 `terminal-wire-node-1.log` 실행은
Node VM 모듈 활성화 옵션이 빠져 테스트 8개 파일을 로드하지 못했다.
기존 명령의 `--experimental-vm-modules`로 바로잡고 원본 실패 로그를 유지했다.
이 검사는 mock 경계와 JS 계약 증거이며 새 WASM 실제 실행은 아직 남아 있다.

`core-full-4.log`의 전체 코어 시도는 exit101이다. 기존
`audio_rt_alloc` 테스트가 종료 후 큐 소비 수를 2로 고정해 실제 3과 달랐다.
해당 fixture의 기대값을 실제 consumed=3/applied=1/expired=2/pending=0과
zero-end consumed=1/applied=0/expired=1로 수정했고, allocation·PCM·고정 상태
검사는 유지했다. 이 수정의 재실행은 아직 남아 있으며 전체 코어 PASS를
주장하지 않는다. `terminal-app-1.log`의 Rust 통합 검사는 진행 중이다.

후속 `core-full-5.log`는 allocation fixture 7개를 통과했지만 기존
start-gate zero endpoint 소비 기대값에서 실패했다. 실제 시작 ACK가 없다는
조건은 유지하고 consumed=1/applied=0/expired=1로 수정했다.
최종 `core-full-6.log` 전체 검사는 exit0, 64개 suite에서 547개 통과,
실패·ignored 0이다. `terminal-native-telemetry-1.log`도 실제 publication과
39-field roundtrip 및 동시 tuple 일관성 검사 9개가 exit0으로 통과했다.

`terminal-app-1.log`는 새 테스트의 PCM byte capacity 오류로 실패했다.
8개 f32 샘플에 필요한 정확한 32바이트로 fixture를 수정한 뒤
`terminal-app-2.log`는 전체 대상 실행을 마쳤다. finite frontier 7개,
public settlement 2개, cohort 15개, 실제 CLI 9개, native preparation 9개가
통과했지만 솔로·Step의 새 fixture 3개는 실패했다. 솔로 경합을 첫 실제
입력 발행까지 유지하고, stopped/fenced 이후 capture export를 수행하도록
수정 중이다. 공개 Step은 endpoint 이후 command target을 admission 전에
필터링하므로 가짜 pending ACK를 만들지 않는다. 실제 pending 감소의 보존
관계는 실제 Mixer 보고서를 사용하는 공유 validator 테스트로 검증한다.
앱 전체 PASS나 실제 브라우저 PASS는 아직 주장하지 않는다.

`terminal-browser-output-1.log`는 잘못된 Rust module filter 때문에 0개를
실행했으므로 검증 근거로 세지 않는다. 실제 module인 `worklet_audio::`로
수정한 `terminal-browser-output-2.log`는 actual Worklet→58-word decode,
terminal expiry, unavailable·marker·grid 거절을 포함한 6개가 exit0으로
통과했다. main/audio WASM 재빌드와 bindings 재생성은 진행 중이다.
솔로·Step 외부 fixture와 실제 pending의 공유 validator fixture는 수정 후
동결됐으며, 그 수정의 실행 결과는 아직 남아 있다.

## 시작 시점 입력 분류의 개발 증거

후속 `terminal-app-3.log`는 7개 대상에서 77개 통과, 실패·ignored 0으로
종료했다. `terminal-internal-pending-1.log`의 공유 완료 validator 검사도
실제 Mixer pending 감소를 포함해 3개 통과했다. 수정 전 시도 로그는 유지한다.
이 결과는 전체 앱 테스트나 물리 출력 검증을 뜻하지 않는다.

`terminal-browser-export-3.log`의 이전 실제 WASM 검사 28개는 통과했지만,
실제 Chromium 개발 실행 `development-1`에서는 시작 직후 `BeforeOrigin`
오류가 발생했다. HOST 기준 시작 이후 입력이라도 실제 audio observation
pair로 매핑하면 logical origin 이전일 수 있었다. 공통 AudioAuthority에서
원본 입력의 검증된 매핑 결과로 이를 분류하고, 판정·소리·capture 없이
입력 chronology만 전진하도록 수정했다. 브라우저 touch의 위치 투영과
native 키 소유권 처리보다 먼저 같은 분류를 적용한다.

`audio-origin-app-1.log`는 실제 Rust 검사 4개 대상에서 60개 통과했다.
빌드 시간은 2분 10초, 테스트 실행 시간 합계는 약 0.38초였다.
`audio-origin-worker-1.log`는 174개 중 173개 통과했으며, 남은 local fixture가
touch를 포함한 계획에 `physical` 모드를 지정한 오류를 수정했다.
새 main/audio WASM과 bindings 재생성은 모두 exit0으로 끝났다.
그 결과로 실행한 `audio-origin-export-1.log`는 30개 중 28개 통과했고,
새 capture 검사 2개의 byte-offset 기대값 실패는 원본 형식과 대조 중이다.
`audio-origin-worker-2.log`의 집중 Worker 검사 19개는 exit0으로 통과했다.
새 WASM 테스트의 원본 keyboard metadata 오프셋과 local 2·4명별 lane
바인딩 fixture를 원본 계약에 맞춘 `audio-origin-export-3.log`는 30개 전부
통과했다. negative touch가 contact 소유권이나 다음 입력의 투영에 영향을
주지 않는 실제 owner 검사도 포함한다.

Chromium `development-2`에서는 실제 시작 오류 없이 section 완료·판정·PCM·
원본 capture 검사를 통과했지만 renderer 진행 검사에서 실패했다.
dynamic packet은 등록된 mode를 상속하므로 자체 `mode` 필드가 없는데
runner가 그 필드를 읽어 실제 drawn 프레임을 전부 제외한 오류였다.
동일 generation/content/sequence의 실제 drawn receipt에서 live mode를
확인하도록 검사만 수정했다. 원본 song timestamp 일치와 서로 다른 진행
프레임 2개 이상 조건은 유지한다. `development-3`에서 1/2 배속은 실제
keyboard 판정 1회, capture 입력 108개, 실제 drawn 프레임 185개와 PCM
440 Hz 검사까지 통과했다. 다음 1/1 배속에서는 section과 capture는
완료됐지만 fixture note가 hit 0/miss 1이어서 실패했다. 원본 capture의
마지막 입력은 song 380.391ms여서 500ms 노트의 450–550ms 판정 구간에
입력 자체가 없었다. runner가 `Playing` 표시 후 벽시계만으로 입력 기간을
끝낸 문제로, 실제 관측된 같은 playId의 song 진행이 750ms에 이를 때까지
원래 CDP Down/Up을 보내도록 수정했다. 별도의 최대 대기시간·마지막 KeyUp·
원본 판정 범위·hit 요구는 유지하고 실제 capture의 노트 구간 coverage
검사를 추가했다. `development-4`에서는 네 배속의 실제 판정·capture·PCM·
drawn 검사와 저장 기록 및 1ns section 완료까지 통과했다. 다음 saved replay
검사에서는 observer가 문자열 playId를 저장하고 숫자와 strict 비교해
실제로 존재하는 원본 bytes를 찾지 못했다. 같은 문자열 표현으로 비교하도록
수정했고 원본 SHA-256 일치 조건은 유지한다. `development-5`에서는
다른 배속으로 실제 saved replay 재생과 원본 hash 일치, 같은 배속 opponent
허용·다른 배속 거절까지 통과했다. local 전환에서는 runner가 Stop 버튼의
비활성화만 보고 closing을 idle로 판단해 roster 변경을 너무 일찍 보냈다.
실제 idle controls의 재활성화와 roster 생성·Discover 활성화를 기다리도록
runner를 수정했다. `development-6`은 첫 1/2 배속에서 실제 AudioWorklet
terminal status 6 (context chronology)으로 실패했다. console/network 오류는
없었으며 마지막 원본 output reports와 terminal 진단을 조사 중이다.
앞선 source/hash 증거와 실패 시도를 유지하고 전체 브라우저 PASS를 주장하지
않는다. 이 실제 출력 오류는 runner 기대값을 완화해 통과시키지 않는다.
`audio-origin-app-windows-1.log`와 `audio-origin-app-macos-1.log`의
앱 `--all-targets --features webtransport` Rust 타입 검사는 둘 다 exit0으로
끝났다. Windows 1분 47초, macOS 1분 28초였으며, xcheck의 외부 C/C++
stub 경로이므로 실제 OS 실행·링크·물리 장치 증거로 세지 않는다.
오디오 보고서 연결은 `bdcf447`에 커밋했다.
원본 입력 분류와 finite 완료 연결은 `30fef2c`, native 실행 파일 연결은
`3a107da`에 커밋했다. 후속 CLI 집중 검사도 실제 9개 통과, 실패 0이다.

후속 `development-7`에서는 context chronology 오류가 재현되지 않았고
종료 진단 목록도 비어 있었다. 한 번의 성공으로 앞선 실제 오류가 해결됐다고
판단하지 않는다. Worklet이 이미 제공하는 terminal 진단을 읽기 전용으로
수집하도록 runner를 보강했다. local discovery는 접힌 Local players
details를 먼저 실제 summary 클릭으로 펼치도록 바로잡았다.
`development-8`은 local section 완료까지 도달했지만 원본 local 입력
acquisition 검사에서 실패했다. 입력을 시작 표시 직후 보내는 대신 실제
관측 song 200ms 이후 보낸 `development-9`에서는 local keyboard acquisition은
통과했고 touch acquisition은 실패했다. touch를 보내기 전에 실제 canvas를
viewport에 스크롤하고, 실제 접촉 좌표가 viewport 내부인지 확인하는
`development-10`은 exit0으로 완료했다. cold refusal, 네 배속의 실제 원본
입력·판정·PCM·capture·drawn, saved-record, 1ns interval 완료, 다른 배속의
saved replay 원본 hash 보존, opponent 허용·거절, keyboard/touch local capture,
Stop/retry, 390/768/1200px 화면 및 strict console/network까지 14개 검사가
모두 통과했고 unmet은 비어 있다. 이는 coordinator 개발 검증이며 독립
qa-browser의 완료 증거를 대체하지 않는다.
판정·provenance·실제 touch capture 조건은 완화하지 않는다.

## 남은 범위

이 문서는 개발 중간 증거다. 현재 태스크의 독립 code/security review와
CLI/browser/desktop QA가 완료됐다는 주장이 아니다. 고정 실시간 배속의
전체 연결, 실행 중 배속 변경, retained practice/seek/loop 배속, 역재생과
물리 장치 검증은 각각 남은 범위에 따라 계속 구현·검증한다.
WBS는 91/193 (47.15%)를 유지하며 전체 BMS player Goal은 미완료다.
