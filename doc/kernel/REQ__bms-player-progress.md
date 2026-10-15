# BMS player 구현/검증 현황 — 2026-10-15

현재 전체 TODO/진행률은 [전체 WBS](REQ__bms-player-work-breakdown.md)를 사용한다.
`python3 tools/wbs_status.py`가 말단 작업의 완료·검증 대기·구현 중·장비/정책 대기와
원래 상위 요구사항 집계를 함께 보여 준다. `--in-progress`는 실제 진행 단위 W/V/N을
보여 주며 각 단위의 다음 gate는 WBS에 기록한다. `--ready`는 부모 범위와 상속된
선행을 충족한 다음 작업을 확인한다.
아래60개 행은 stable 기능 ID와 과거 source survey이며 완료 분모가 아니다.

현재 runtime05.13.06은 구현·연동 중(W)이다. 268bb73d의 독립 DEEP code review
FIX_NOW5와 별도 security의 중복2건을 수정했다. c32d1287은 native optional focus,
caaaba03은 Window session 키·기록 보관 예약/Save·편집 중 key-up,
f9243be5는 browser/native Records의 가변 조건 역사 조회와 별도 고정 상대 검사를
반영한다. 현재 관련 Node368/0·Rust 기록 조회10+34+23=67/0이 통과했고,
focus 전용9+15=24/0도 통과했다. 서로 겹치는 검사는 합산하지 않는다.
실제 release UI/audio WASM과 즉시 matching bindgen 빌드도 통과했다.
새 게임 메뉴 Records·editor release 흐름을 포함한 실제 browser 재검증과
새 독립 code/security review, browser/CLI/desktop QA는 남아 있다.
전체136/489·오디오 커널12/27을 유지하며 개발 증거를 독립 QA PASS로 표시하지 않는다.

리뷰 전 개발 통합 checkpoint: 실제 브라우저 필수12개 흐름이
모두 통과했다. 열세 번째 실행80375는 추가 저장 검사까지13개 check PASS,
unmet0·console/network0·owned browser/Xvfb exit0이다. ACK 뒤 새 poll을 기다리는
Worker 수정과 정확한 retirement 거절 처리의 Node324/0·실제 Rust Worklet24/0을
확인했다. 브라우저는 현재 JS와 일치하는 기존 fresh UI/audio WASM을 사용했다.
이 증거는 해당 버전의 개발 검증이다. 위 리뷰 수정 뒤 새로운 독립 code/security
review와 browser/CLI/desktop QA가 필요하며 현재 source 전체의 QA PASS로 해석하지 않는다.
[현재 변경/증거](../changes/2026-10-15-runtime-playback-rate.md)를 따른다.

아래 날짜별 문단은 당시의 실행 기록이다. runtime의 현재 상태는 위 checkpoint가 우선한다.

2026-10-15 실제 browser 첫 실행: fresh 두 WASM은 일치했으나 cold capability가
정상적으로 빈 값인 시점에 JS가16words를 요구해 시작이 실패했다. network/console
오류는 없었고 owned browser/Xvfb는 exit0으로 정리했다. 기존 Rust의 실제 Started
이후 권한 조건은 유지하며 JS의 nullable capability·Ended/fresh 분리를 수정했고 관련 회귀41/0을 확인했다.
두 번째 실제 실행은 cold 준비를 통과했으나 pre-arm 실제 관측을 버려 Started0을
검증할 실제 bracket이 사라졌고 HistoryExpired로 종료했다. 원본 signed 시계 쌍을
보존하는 공통 authority와 retained JS 연결을 구현했다. core7/0·앱 통합28/0·기존
authority 회귀216/0·Node95/0과 새 두 release WASM·matching bindgen을 확인했다.
실제 세 번째 실행은 시작·노트 판정을 통과했고 네 번째의 유한 trusted 입력
한 쌍에서도 voice_full이 발생했다. 반복 입력만의 문제라는 가설은 반증됐다. 실제
전체 슬롯4096을 BGM에 전부 예약한 browser cold 예산 혼동을 확인해, 기존 full
geometry overlap을 재사용하는 공통 예산 adapter를 수정했다. 실제 solo/local 키음·BGM
PCM·배속 전환과 cold 원자성 통합32/0, 새 두 release WASM·matching bindgen을
확인했다. 실제 일곱/여덟 번째는 저장·불러오기·독립 replay를 포함한7개 검사를
통과했다. 여덟 번째 loop region의 NegativeElapsed는 정상적인 무진행 frontier를
새 attempt로 먼저 투영한 순서 오류였다. Step 통합34/0과 native Source/Target
solo/cohort·correspondence 회귀51/0 및 새 두 release WASM·matching bindgen을
확인했다. 실제 아홉 번째는 loop·자율 기록 회수·disable-loop까지 진행했다. 이어
짧은 구간의 정상 종료 뒤 runner가 숨겨진 scrub UI를 조작하려 해 멈췄다.
진단 로그의 반복 CDP 복사를 줄인 열 번째 실행은 loop·disable-loop·scrub·F5까지
진행했으나 retired File custody의 ACK 순서 검사에서 실패했다. 실제 제품은
Window ACK 뒤 Worklet ACK를 보냈으며, 후등록된 진단 listener가 수신을 한 tick
늦게 기록한 문제를 확인했다. 실제 수신 진입 관찰을 수정한 뒤 재검증한다.
아홉/열 번째 console/network 오류는0, owned browser/Xvfb는 exit0이었다.
열한 번째에도 capture 옵션만으로 관찰 순서는 해결되지 않아 listener를 첫 await
앞으로 옮겼다. 열두 번째는 초기 배속 전환 뒤 pause와 history-retire 경합에서
실제 retained-admission 거절로 플레이가 종료됐다. incomplete File4060B는 보존됐고
console/network0·owned browser/Xvfb exit0이었다. 이 실제 경합의 수정과 전체 browser
검사·독립 review/QA는 남는다. strict ERR_ABORTED는 두 번째 실행의 과거 관측이며
별도 browser-audio acceptance의 미완료 범위를 해소한 근거는 아니다.

2026-10-15 최신 runtime 연동 검사: 공통 native Source/Target pump·정확한 Source cut과
미투영 경계 barrier·6곳 native cold 조립은 앱 lib3356/0(ignored9)·관련 integration99/0을
통과했다. Windows/macOS C-stub 타입 검사와 실제 두 release WASM·matching bindgen도
통과했다. Worklet·포트 Node231개, Worker/Window 신규33개와 관련 기존504개도
모두 통과했다. 실제 poll 중 요청의 wake와 잘못된 최종 File 응답의 보관 경계도
회귀로 확인했다. 추가 PCM·기록·리플레이 end-to-end3개도 모두 통과했고 실제 브라우저
runner는 열두 차례 실제 실행했고 남은 흐름을 재검증 중이다. 실제 browser·desktop와 전체 독립 review/QA는 남으므로
WBS 완료 수136/489, 오디오12/27을 유지한다.

2026-10-15 실행 중 배속 전환05.13.06은 W다. 공유 phase·PCM·exact Transport
history·가변 source 기록의 AC001–004를 구현하고 core 전체816/0·app 관련81/0을
확인했다. 앞선 scoped 검사는 subset이므로 더하지 않는다. 이어서 Worklet public24/0·
library24/0, typed mailbox30/0과 browser-audio WASM check가 통과했다. 이어서 공통
owner10/0·capture12/0·SessionLaunch7/0·lineage5/0, browser retained 준비5/0·
timing policy7/0, public native control11/0·retained RateView8/0·실제 desktop
제어14/0·IME3/0을 확인했다. 겹치는 library 검사 수는 더하지 않는다. 현재 공통
native pump·browser Step/WASM owner·실제 native6곳 runtime 시작 조립과 browser JS
수직 연결을 구현해 위 범위의 개발 검사를 확인했다. 전체 strict browser acceptance와
독립 review/QA는 남는다.
[runtime 배속](REQ__runtime-playback-rate.md)과
[실제 변경/검증 범위](../changes/2026-10-15-runtime-playback-rate.md)를 따른다.
전체 runtime 지원 완료가 아니므로 완료 수/분모136/489는 유지한다.

2026-10-15 현재 browser audio acceptance: `83330c7c`의 실제 두 WASM identity/
extent 검증과 `53c18114`의 page-owned CDP cleanup 수정은 독립 DEEP code/security
검토를 통과했다. 앞선 전체 Node1516/0과 마지막 변경의 별도 CLI 검사는 PASS다.
실제 browser13journey는 두 번 끝까지 실행했지만 strict ERR_ABORTED가 남고,
두 번째 실행의 cleanup error는0이다. 설치된 Chromium153의 공식 소스가 matching
upstream patch 이전 loader를 포함한다. fixed-engine strict acceptance는 미완료이며
기존 D/V/W/E·분모는 변경하지 않는다. 05 오디오 커널은12/27, 07 실제 플레이·출력
교체는24/102이며 전체136/489를 유지한다.
[현재 증거와 한계](../changes/2026-10-15-browser-audio-current-acceptance.md)를 따른다.

2026-10-15 ASIO .14.01–.06은 V, .14.07은 W다. 이전 독립 review의3건을 a56e039b·8ee68fb1·272b564f에서 수정했고 최종 DEEP code/security 검토와 단일 독립 소프트웨어 QA가 실제 PASS했다. QA는 core749/0, platform lib507/0, featureful app lib3319/0·bins670/0, Source allocation1/0·CLI19/19와 foreign typing/C++ 검사를 실행했다. native21·adapter17·Windows composition1은 typed-only이며 실제 SDK/driver .14.08은 E다. 완료 표시는 hook-owned 최종 gate를 따르고 전체136/489(27.81%), 남음353·원래91/193을 유지한다. [진행 기록](../changes/2026-10-15-asio-retained-source-output.md)을 따른다.

2026-10-14 WBS 세분화: 기존 미완료117개를383개 구체 실행 단위로 나눴다.
기존 완료106개와 원래 요구193개/완료91개는 유지한다. 기존에 실제 검증된 하위
범위29개를 드러내 새 말단 기준선은135/489(27.61%), 남음354개다. 이는 분모를
바꾼 기록이며 분해 전106/223(47.53%)에서 진행이 후퇴하거나 새 기능29개가
추가된 것은 아니다. 현재 상태는 WBS 집계 명령을 따른다. 각 하위 행에 범위,
상태, 실제 근거 또는 다음 검사, 선행 조건을 남겼다.

2026-10-14 CoreAudio E75(진행 중): .15를12개로 분해했다. 공통 retirement CAS,
cold history 성장, portable production callback의3개 software 범위는 각각 실제
검사로 D다. native 소유권/실패 fixture·UI·solo/local은 V, backend는 리뷰 수정으로 W이며 이후 결합
review는 W·QA는 N, 실제 Mac/HAL acceptance는 E다. b8f7494f의 native audio48개·추가
presentation6개와 app backend fixture21개를 포함한 macOS C-stub typing은 exit0이며
실제 native 실행 PASS가 아니다. platform 전체 library469/0/ignored2와 host 공통
coordinate_source21/0을 실행했다. d3fc5fb6에 backend를 커밋했고, 실제 runner
조회에서 owned Mac 없음·macos-host 누락을 확인해 native acceptance를 별도로 남긴다.
83b50487의 typed UI도 macOS lib/tests/bin 형 검사 exit0과 공통 request9/0을
확인했다. 9e68bbb4의 solo/local complete 연결과32c5b78d의 실제 callback 크기
converter 준비·등록 실패 회귀도 macOS typing exit0이다. host platform473/0/ignored2를
실행했고 native audio52개·추가 presentation6개는 여전히 typed-only다.
consumer8/0·공통 출력 pump9/0·Source owner9/0과 CLI 도움말/오프라인 PCM fixture도
실행했다. 최종 독립 review/QA와 native 오디오 이력의 실제 시간/장치 acceptance는 아직 미완료다.
2c14803a의 독립 DEEP code review는 정상 future native 관측 대기와 history/승인
경합2건으로 FAIL, 별도 security review는 PASS다. AC012로 shared replacement port·
고정2개 typed 관측 대기·narrow classifier와 회귀를 수정했다. 2026-10-15 최종
host 전체 app library3279/0/ignored8(새 회귀19개 포함)과 macOS platform/app
lib/tests/macOS bin C-stub typing exit0·Rust40 format/diff를 확인했다. backend는
V로 돌아갔고 cfg-Mac backend24개는 typed-only다. 새 정식 리뷰와 QA는 아직 남는다.
2026-10-15 frozen2a7313c6의 새 정식 code/security review는 각각 PASS0이고,
그 뒤 독립 CLI QA도 software scope PASS다. core749/0·platform473/0/ignored2·
format23/0·app3279/0/ignored8·bins640/0/ignored17·Source allocation1/0과
WASM/Windows/macOS typing, source Clippy·CLI17/17을 확인했다. 외부 WASAPI120과
중복 consumer 실행은 고유 coverage에 더하지 않는다. .15.10 검토는 D이며 .15.11은
독립 문서 재검토 PASS 후 owning verification 대기 V, 실제 Mac/HAL acceptance는 E다.
한 번 task_verify를 실행했지만 hook-owned receipt가 없어 PENDING이다. 현재
136/489(27.81%), 남음353이며 실제 리뷰/QA PASS가 task close·전체 플레이어
완료를 뜻하지 않는다.
[현재 변경](../changes/2026-10-14-coreaudio-retained-source-output.md)을 따른다.

2026-10-14 좌표 경계 E74: frozen7ac7a1af의 독립 DEEP 리뷰 PASS0 다음 독립 CLI
QA가 DTO 이동의 소프트웨어 범위 PASS했다. complete core746·platform434/0/ignored2·
format23, 전체 app3260/0/ignored8·bins624/0/ignored17, host bins·browser-feature
WASM·Windows/macOS C-stub typing·Rust15 format/diff·문서 링크139·공개 CLI12/12가
통과했다. coordinate-epoch13개도 포함되며 focused 수는 합산하지 않는다.
feature Clippy는 exit0/app394·platform111 warnings이고 strict-clean이 아니다.
foreign typing/CLI smoke는 실제 native 재생 증거가 아니다. 도구 자동 설치 위반은
정확한 신규 경로만 원복했고 변경 문서에 기록했다.01.05/.06은 W,106/223·원래
91/193 그대로이며 문서 audit·owning verify/close 및 전체 player 완료는 별도다.

2026-10-14 좌표 경계 E73(개발 당시): 순수 ConvertedBoundaryFacts를 기존 core audio로 옮기고
platform의 같은 타입 직접 재노출과 application/port10개 core 참조를 연결했다.
core 독립 check·경계7·mixer16·platform434/0/ignored2·public-path 호환 포함 format23,
app target_327/0/ignored1, host bins·browser-feature WASM·경고 비차단 Clippy와
Rust15 format/diff를 통과했다. 필드/default/생성 증거 의미와 native 실행문은 그대로다.
Windows/macOS C-stub Rust typing도 exit0이며 실제 native 재생은 아니다.
독립 최종 리뷰·전체 앱 QA·문서 audit는 남으며01.05/.06은 W,
전체106/223·원래91/193을 유지한다.
[변경과 검증 범위](../changes/2026-10-14-core-converted-boundary-facts.md)를 따른다.

2026-10-14 준비 경계 E72: frozen2b9928d3의 독립 DEEP 리뷰 PASS0 다음 독립 CLI QA가
소프트웨어 범위 PASS했다. 전체 app3260/0/ignored8·bins624/0/ignored17, 준비7·
retained2·cohort7, host bins·browser-feature WASM·Windows/macOS C-stub typing과
scoped format/diff·문서 링크139·공개 CLI12/12가 통과했다. focused 수는 중복이고
394 warnings의 Clippy는 strict-clean이 아니다. CLI smoke/typing은 실제 native
재생을 입증하지 않는다.01.05/.06은 W, 전체106/223·원래91/193 그대로이며
자동 완료 기록·owning verify/close와 전체 player 완료는 별도다.

2026-10-14 준비 경계 E71(개발 당시 기록): 세 OS의 selected-chart 키음 필터를 기존
local_preparation의 순수 함수로 통합했다. 실제 준비7·retained-chart2·cohort-setup7이
통과했고, 전체 completion과 원본 source/PCM/BGM/키음 복원·뒤로 이동을 검증한다.
호스트 bins 및 Windows/macOS C-stub typing은 exit0이며 실제 native 실행은 아니다.
독립 최종 리뷰와 전체 앱/browser-feature 검증은 다음 gate다.01.05/.06은 계속 W,
전체 **106/223(47.53%)**, 원래91/193을 유지한다.
[변경과 검증 범위](../changes/2026-10-14-native-cohort-selected-sound-preparation.md)를 따른다.

2026-10-14 exclusive E70: frozen bd9b8916의 독립 DEEP 리뷰 PASS0 뒤 독립 CLI
소프트웨어 QA가 PASS했다. app3258/0/ignored8·bins624/0/ignored17·platform434/0/
ignored2·worker37·외부100·sizing22·Source/Target 할당 각1, WASM·foreign typing·
경고 비차단 Clippy·Rust12 format/diff·문서 링크144·공개 CLI26/26와 동시 render를
통과했다. fixture 중복은 합산하지 않고 Windows 신규9개는 typed-only다.
순수 worker 범위 .13.01만 V→D로 옮겨 **106/223(47.53%)**, D106·V72·W20·N2·
E15·P5·U3이다. 원래91/193과 exclusive 부모의 전체 범위는 유지한다.
새 Windows QA도 실제 COM Render/AllStates 열거/GetCount가 성공했지만 endpoint0,
로그인 콘솔/현재 player 부재로 BLOCKED_ENV다. VM은 같은 incarnation의 초기Off로
정상 복구했다. native .13.02/.03은 V, 실제 재생 .13.04는 E이며 task/Goal 완료가 아니다.

2026-10-14 exclusive E69: 사전 독립 coverage 점검에서 확인한 shared cold-helper
우회 분기를 실제 pending6/buffer4·shared3조합의 owner 보존 회귀로 보강했다.
최종 공통 worker37·외부 worker binary100 PASS이며 중복 fixture는 합산하지 않는다.
platform/app 경고 비차단 Clippy와 실제 windows_bms 도움말도 exit0이다.
독립 정식 review/QA와 native 실행은 남고 E68의 상태/완료105/223은 유지한다.

2026-10-14 exclusive E68: native complete 연결 뒤 Windows platform/app lib/tests
형 검사 exit0, 공통 Source Owner/UI/pump37·Source/Target publication 할당 각1과
scoped Rust12 format/diff를 확인했다. 새 Windows9개 fixture는 typed-only다.
native/consumer2개 W→V로 D105·V73·W20·N2·E15·P5·U3,105/223(47.09%)이며
독립 최종 review/QA와 actual Windows 장치 실행은 남는다. 원래91/193은 그대로다.

2026-10-14 exclusive E67: 실제 공통 worker36·sizing22·외부 worker binary99와
Windows bin79를 통과했다. 기본 endpoint 선택의 실제 실패2개를 재현하고 공통
parser를 수정해 GREEN2를 확인했다. 새 exclusive 측정 경계 heap 호출은0이다.
외부 binary의 중복 fixture는 다른 suite에 합산하지 않는다. portable .13.01은 V,
native .13.02는 작성 중 W다. D105·V71·W22·N2·E15·P5·U3,105/223(47.09%)이며
독립 review/QA와 actual Windows 실행은 아직 남아 신규 완료로 계산하지 않는다.

2026-10-14 exclusive E66: 같은 mode family의 WASAPI exclusive retained 교체를
기존 complete Owner/worker로 확장하는 구현에 착수했다. 원래 .13의 범위를 유지하고
portable worker·native lifecycle·Windows consumer·actual device acceptance의4개
하위로 나눠 활성 분모223개다. **105/223(47.09%)**, D105·V70·W22·N3·E15·P5·U3이며
신규 완료는 없다. 원래 제품 요구91/193(47.15%)은 그대로다. 실제 full-buffer lease와
whole-multiple pending PCM 보존·Exact/actual geometry를 먼저 계약화했다. 아래 E65의
완료105/분모220은 당시 기록이며 현재 상태는 E66과 WBS 도구가 우선한다.

2026-10-14 Source E65: frozen9b6c5ae4의 독립 DEEP 리뷰 PASS0 다음 독립 CLI QA가
소프트웨어 범위 PASS했다. desktop,webtransport app3258/0/ignored8·bins610/0/ignored17·
platform415/0/ignored2·sizing22·worker 외부81·Source/Target 할당 각1, WASM·foreign
stub typing·scoped48 Rust format/diff·경고 비차단 Clippy와 공개 CLI18경로를 통과했다.
중복 fixture는 합산하지 않고 Clippy 경고도 남는다. 범위가 소프트웨어인8개만
V→D로 옮겨 현재 **105/220(47.73%)**, D105·V70·W20·N3·E14·P5·U3이다.
Source8D·4V·3N이며 원래 제품 요구91/193(47.15%)은 그대로다. 실제 Windows VM을
부팅해 native Render endpoint 열거는 성공했으나0개였다. 로그인 콘솔과 현재
실행 파일/빌드 도구도 없어 desktop QA는 BLOCKED_ENV다. VM은 초기Off로 복구했다.
native .05/.10/.11과 .11을 선행으로 갖는 .12는 V다. hook-owned 완료 증거가 없어
한 번의 task_verify도 PENDING이며 task close/전체 Goal 완료로 처리하지 않는다.
아래 E64 이전 문단은 각 당시의 검사/상태 기록이며 현재 집계는 E65가 우선한다.

2026-10-14 Source E64: 리뷰3건을 ef72c8d2·154704a4로 수정했다. actual RED2→GREEN,
platform415·sizing22·worker external81·Source UI9·Windows bin72·전체 app2774를
통과했다. Windows native5개/18조건은 새 형 검사만 통과했으며 미실행이다. 현재
Source12V·3N이며 새 독립 DEEP 리뷰/QA 전 완료로 계산하지 않는다. WBS D97/220과
원래91/193은 유지한다. 아래 E63은 수정 전 실패 기록이다.

2026-10-14 Source E63: 독립 DEEP 정식 리뷰에서 cold Duration 거절/Exact 표현
검증과 setup MMCSS 정리 오류 보존3건을 확인해 worker/native/Windows consumer를
W로 되돌렸다. 현재 Source9V·3W·3N이며 수정과 독립 재검토 전 완료로 계산하지
않는다. 아래 E62 검사 수는 그 이전의 실제 개발 근거다.

2026-10-14 Source 후속: 실제 WASAPI는 SourceCounter임을 확인하고 portable
Frame/Counter 기초와 shared WASAPI complete owner 경로의 계획 검토를 마쳤다.
원래07.08.07을15개 실행 단위로 나눴다. E61의 실제 연결에서 startup origin
결함을 찾아 controller를 W로 되돌린 뒤 E62에서 수정·실제 회귀를 통과했다.
현재 native/pause/state/port·worker·COM·controller·Owner/adapter/UI·Windows
consumer/공통 pump12개가 V, exclusive/ASIO/CoreAudio 후속3개가 N이다.
직전 bounded 검사 portable app2773·연결4·Windows bin69·UI8·platform411·Owner24·
canonical packet11·Source controller21·worker14와 Source/Target publication·worker
외부 할당 각1 검사를 통과했다. typed 오류/authorization 경합은 실제 RED1건 뒤
분류를 수정해 GREEN을 확인했다. cached Windows lib/tests typing은 실제 COM/장치
실행이 아니다. 새 연결4 RED 뒤 production 원점 계산과 1ns tamper 거절을 수정해
실제 연결4/전체 회귀를 통과했다. PCM 전체 frame 회계와 종료 silence·원본
capture codec/hash·member/attempt를 실제 데이터로 확인했다. 독립 최종 검증과
실제 Windows 실행은 남으며 Target의 유사 startup 가정은 별도 감사 대상이다.
기존 browser WASM도 통과했으며 겹치는 필터는 합산하지 않는다.
활성 분모220·완료97(44.09%)과 원래 제품 요구91/193(47.15%)은 그대로다.
새 검사는 독립 최종 review/QA나 실제 Windows/전체 Source 완료가 아니다.
Legacy 회수, uncertain admission retry 격리와 실제 Source 좌표를 먼저 계약으로
기록했으며 ASIO/CoreAudio/exclusive/native/browser/성능/출시는 계속 남는다.

2026-10-14 현재 진행 추적: 원래 제품 요구 집계는 **91/193(47.15%)·조건부4개**를
유지한다.07.08/07.09는 원래 범위를 보존한 구조 부모 아래10개/5개 실행 단위로
분해했고 확장 말단 집계는 위 도구가 계산한다. 여섯 D는 이미 커밋되고 검사된
coordinate 기초와 Linux encoding의 bounded software 범위를 드러낸 것이며 새 제품
구현 완료가 아니다. current AC004 controller는 앞선15 PASS/3 FAIL 후 수정하고
실제로 재실행해18 PASS/실패0을 확인했다. 후속 borrowed commit의18 unit·1외부
할당 검사와 관련 회귀도 통과했고 실제 publication의 새 할당0회를 확인했으나
최종 독립 gate가 남아 V다.
새 retained Owner/UI(AC005)는 자체 새16+기존1 테스트, 관련 회귀와 모든 bin check를
통과했다. 공통 pump(AC006)도 실제 solo·2인 cohort·전환상태3개와
pure app2674·할당1·desktop app3129/각 bin/check를 통과했다. 이후 `015256a3`의
독립 DEEP 리뷰가 native authorization과 history-retirement 사이 경합1건 및 연결
fixture의 no-op history port를 확인해 두 단위를 W로 되돌렸다. 이후 기존 API의
실패1건을 재현하고 conditional history 서비스 및 실제 연결 port를 수정해
요청/관측25·실제 연결3·전체 pure app2682·할당1 검사를 통과했다. 두 단위는
E54의 V로 돌아갔다. 이후 E55에서 독립 DEEP 재리뷰/CLI QA가 PASS했고 실제
desktop/WebTransport lib3166·bin584·platform387·외부 할당1·WASM 및 foreign stub
typing/포맷 검사를 확인했다. 실제 GUI는 Settings Apply/reopen·solo/local 시작을
시도했으나 입력/오디오 장치가 없어 native 재생 전 BLOCKED_ENV다. acknowledged
pause/Output/resume/capture 및 자동 완료 기록은 남았고 전체 Goal은 활성이다. 개발 검사 결과를
독립 PASS로 바꾸지 않으며
controller·Owner/UI 결과를 그 경로의 증거로 물려받지 않는다. Source/scalar/finite/foreign/
실제 paused GUI·물리/applied 전체 기준은 각 child에 남는다.
Target retained replacement task는 실제 장치 blocker를 기록해 parked 상태이며,
전체 Goal은 활성인 채 Source 후속 child의 COM/controller 병렬 구현을 진행한다.
WBS 작업은03435bf4에서 실제 독립 review/CLI QA를 통과했고 자동 검증 기록 누락으로
종료만 parked 상태다. 이 추적 작업의 검사를 제품 task의
formal review/QA나 전체 Goal 완료로 바꾸지 않는다. WBS의 E46–E55와
[이번 변경 기록](../changes/2026-10-14-wbs-ongoing-work-breakdown.md)을 따른다.

2026-10-11 현재 연결 근거: Records의 genuine preview/history 및 retained Own/Other
경쟁 선택은 WBS 11.05/E42의 독립 실제 browser·UX·CLI 검증과 원래 기준 감사를
마쳤다. 당시 원래 요구 집계는91/193(47.15%)다. 관련 기능은 BK-032/BK-038/BK-039이며,
아래 과거 survey 상태를 전체 기능 완료 판정으로 바꾸지 않는다.
[현재 변경 및 검증 범위](../changes/2026-10-11-records-retained-actions.md)를 따른다.
물리 동기화·전체 UI·출시 및 Harness close는 별도다.

2026-10-08 재개 주의: 아래는 이전 revision의 기능 인벤토리이며 현재
완료율/TODO 집계가 아니다. 전용 native input collector는 새 구현과
독립 검증을 마쳤다(REQ__native-input-collector.md, closed task). 실제
same-rate ALSA/app pending PCM 소유권도 REQ__native-output-continuity.md의
새 검증을 마쳤다. 별도 browser renderer Worker·live class/EX·mines·records
경로 역시 최신 source/evidence를 우선한다. TASK__parallel-player-requirements의
범위 표는 WBS에서 역사적 매핑으로 보존한다. 전체60개 행의 새 source 감사는
이번 진행 단위 분해의 범위가 아니며 최신 bounded 근거는 WBS의 각 단위를 따른다.
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
