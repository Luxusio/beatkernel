# CoreAudio retained Source 출력 연결

현재 진행 중이며 구현 완료나 QA PASS 기록이 아니다. 전체 플레이어 요구사항을
유지하면서 CoreAudio의 complete 소프트웨어 소유권, 실제 callback/회수, 기존
backend/typed UI/solo/local 소비자를 연결한다. 원래 Source history identity를
보존하고 첫 미전송 프레임에서 다시 열어, 미리 생성한 PCM을 건너뛰거나 재생성하지
않도록 한다. 사전 [복구 계약](../kernel/REQ__coreaudio-recoverable-open.md),
[Source 교체 계약](../runtime/REQ__retained-source-output-replacement.md),
[아키텍처 결정](../kernel/ADR__native-runtime-boundaries.md)은 ef99f3a3에 커밋했다.

독립 계획 검토는 승인 검사가 outer publication보다 앞서야 한다는 점, complete
telemetry의 first-unsent basis, 전체 채널 검증 후 불가실패 copy, 실제 변수 callback
최대 크기와 immutable Mixer 성장 한계를 확인했다. 기존 WASAPI legacy가 history
준비 전에 telemetry를 만들 수 있으므로, 기존 helper를 전역 변경하지 않고 새로운
CoreAudio complete cold 경로에서만 선택적으로 history capacity를 늘린다.
승인 CAS 추출과 cold history 확장은 서로 겹치지 않는 구현/테스트 작성자 쌍으로
진행한다. 기존 Source Owner/ports/UI/pumps와 legacy 복구를 재사용한다.

Apple의 [AudioDeviceIOProc](https://developer.apple.com/documentation/coreaudio/audiodeviceioproc)
계약에 따라 출력 버퍼는 처음에 0으로 제공된다. 승인된 retirement는 추가 PCM이나
publication 없이 반환하며, 이 침묵을 Source admission으로 세지 않는다.
[가변 buffer 설명](https://developer.apple.com/documentation/coreaudio/audiohardwaredevice/usesvariablebufferframesizes)에
따라 nominal과 실제 최대 callback 크기를 cold 단계에서 구분한다.

## 검증 상태

사전 계약과 독립 계획 검토를 마쳤다. 승인 CAS 기반은 8b9a63e9, 선택적 cold
history 확장은 f8afbef9에 각각 커밋했다. cached Rust1.98.1, offline/locked,
jobs2 환경에서 core mixer_source_playback_history14/0과 platform 전체 library
450/0/ignored2를 실행했다. platform에 neutral slot10·기존 WASAPI worker37·
output state21이 포함되며 중복 합산하지 않는다. 새 테스트는 core3·platform16개다.
platform의 기존 경고3개는 남아 있으며 strict-clean을 주장하지 않는다.
로그는 target/wf/retained-source-coreaudio-output/{ac010-core,foundation-platform}.log다.

portable production callback은 40b8821b에 커밋했고 별도19/0을 실행했다.
첫 실행17/2 중 두 실패는 테스트가 covering interval을 pin에서 쪼갠다고 잘못
기대한 것이었다. 원래 interval descriptor 보존을 확인하고 실제 pause 경계가
생긴 뒤 새 revision/pin으로 complete interval compaction을 검증하도록 보강해
19/0을 확인했다. 로그는 ac003-callback.log와 ac003-callback-fixed.log다.
pending prefix/short refusal, 모든 채널 검증, 원래 native extent, history/practice
pin과 callback의 calibrated zero heap을 검증한다. 방어적 post-copy admission
실패는 infallible copy와 exclusive safe owner 때문에 corruption 없이는 도달할 수
없으므로 인위적인 상태 훼손 hook을 추가하지 않았다.

native 수명주기에 production step을 연결하고 같은 실행 코드의 static operation
주입으로 native fixture44개(신규20·보존24)를 작성했다. cached macOS C-stub의
platform/app lib/tests와 macOS bin 형 검사 exit0을 확인했다. 첫 fixture 형 오류
6건과 외부 WASAPI allocation binary의 neutral module 누락2건은 수정했으며
외부 allocation binary는 host에서116/0을 실행했다. 외부 binary에는 source를
포함한 기존 fixture도 있으므로 library 수와 중복 합산하지 않는다. 로그는
ac004-macos-check-fixed.log / retry1.log / retry2.log이고, host 외부 검사는
`cargo test -p beatkernel-platform --test wasapi_source_worker_allocations --locked --offline -j2`다.
C-stub은 Rust typing만 확인하며 신규 native fixture의 실행 PASS가 아니다.
이 초기 기반 시점에는 backend/UI/solo/local 연결, 독립 코드/보안 검토, 전체 CLI
QA와 최종 문서 감사가 남았다. 이후 연결은 아래 후속 절을 따른다. 이 기반
테스트 통과가 전체 retained 교체의 완료를 뜻하지 않는다.

사용자가 WBS 세분화를 요청해 미완료117개를383개 실행 단위로 나눴다. 기존
완료106개·조건부4개·원래91/193과 모든 원래 범위를 보존했고, 기존 실제 검사
근거가 있는 하위29개를 드러내 새 기준선은135/489(27.61%), 남음354개다.
분해 전106/223(47.53%)과 직접적인 진척 증감 비교에 사용하지 않는다.
세분화 당시 CoreAudio .15는12개로 분해했으며 portable 기반3개 D, native 소유권/fixture2개 V,
backend/UI/solo/local4개와 결합 review/QA2개 N, actual Mac acceptance1개 E다.
각 행에 ID·상태·근거/다음 검사·선행을 기록하고 task AC와 연결했다.
WBS 도구는 기존 중첩 지원을 그대로 사용한다. 실제 CLI4모드, 기존 unittest50개,
기존 행 보존·117개 coverage·부모/선행/순환 검사와 local 링크137개가 통과했다.
독립 문서 근거 감사도 같은 결과를 확인했다. 이 범위의 문서 검증은 전체
CoreAudio task의 formal code/security review·QA·verify/close를 대신하지 않는다.

실제 device broker의 owned/backends 조회에서 owned Mac이 없고 macos-vm은
missing-prerequisites, missing macos-host, providers 없음으로 확인됐다.
Linux production callback 실행, macOS C-stub Rust typing과 실제 HAL/device 재생은
서로 다른 증거다. 실제 registration/start/stop/drain·IOHID·native 관측 진행과
음향/device acceptance는 전체 요구사항으로 계속 남는다.

## native 소유권과 앱 backend 연결 후속

b8f7494f에 Legacy/Complete native owner·등록/시작/정지/해제/회수와 승인 ticket을
커밋했다. 독립 구현 점검은 승인 이후에도 직전 Source 관측이 live로 읽히는
결함을 찾았다. is_started와 Source 읽기 전후, history 접수 경계에 승인 검사를
추가하고 실패한 Start의 public Source/history도 거절한다. 기존 진단 snapshot과
이미 접수한 history 의도는 보존한다. 동일 production Context/Stream의 경합4개
fixture와 failed-Start 검사를 보강했다.

가변 callback이 nominal64보다 커지는 경우 기존 presentation 검사가 이를
거절하는 결함도 고쳤다. 같은 독립 계획 검토자가 필요한 수정으로 확인했고,
실제 cold maximum을 명시적으로 받는 helper를 기존 검사 본문에 추가했다.
legacy helper는 nominal을 넘기며 기존 검사를 유지한다. native/applied nominal을
바꾸거나 원래 Mach/sample/flags/grid/domain 검증을 생략하지 않는다.

현재 native audio fixture48개·새 presentation fixture6개를 포함한 cached macOS
platform lib/tests typing은 exit0(21.98초)이다. actual native 실행 결과가 아니다.
로그는 `target/wf/retained-source-coreaudio-output/ac004-macos-fences-max.log`다.
host platform 전체 library는469/0/ignored2이며 앞선450과 callback19를 포함한
새 전체 실행이다. 로그는 `/tmp/beatkernel-coreaudio-platform-full-14033.log`이고
`cargo test -p beatkernel-platform --lib --locked --offline -j2`로 재현한다.

앱 complete backend는 기존 lifecycle/remix/original/Source/history/conditional port를
구현하고 기존 원래 packet 검사를 공유한다. pending failure의 original/cleanup
오류·matrix·immutable capacity를 유지하며 Source creation basis는 first-unsent다.
complete output이 기존 output 필드를 내부 소유해 UI가 실제 stream을 빌릴 수
있지만, complete 회수는 계속 NativeOutputState 추출을 사용한다. 새9개/기존12개인
앱 fixture21개는 실제 owner의 cold refusal·회수/PCM/history/matrix 보존과 원래
보고서8-frame/prefix3-frame packet을 검사하도록 작성했다. cached macOS app
lib/tests/bin 형 검사 exit0(1분)이며 native fixture 실행 주장은 아니다.
로그는 `target/wf/retained-source-coreaudio-output/ac005-macos-backend.log`다.
backend는 d3fc5fb6에 커밋했고 host 공통 coordinate_source21/0도 실제 실행했다.
명령은 `cargo test -p beatkernel-bms-runtime --features desktop,webtransport --lib --locked --offline -j2 coordinate_source`,
로그는 `/tmp/beatkernel-coreaudio-common-source-14033.log`다. 이21개는 기존 공통
controller/Owner fixture이며 위 cfg-Mac backend21개를 host에서 실행한 것이 아니다.
typed UI는 83b50487에 커밋했다. static Legacy/Complete facade는 같은 실제
stream을 빌려 주고, complete lifecycle/조건부 Source 조회·history는 complete
Owner에 전달한다. 기존 scalar 경로를 complete에 적용하면 명시적으로 거절한다.
실제 maximum을 검사하면서 적용 nominal과 matrix source width를 유지하고,
기존 coordinate Source bridge의 요청/응답·WouldBlock·epoch 의미를 재사용한다.
새 UI fixture7개를 작성했고 macOS lib/tests/bin 형 검사는 exit0(1분10초)이다.
첫 검사에서 새 fixture의 producer mut 누락1건을 고쳐 재검사했다. 로그는
`target/wf/retained-source-coreaudio-output/ac006-macos-ui.log`와
`target/wf/retained-source-coreaudio-output/ac006-macos-ui-retry1.log`다.
이7개는 native 실행 PASS가 아니다. 별도 host 공통 retained_source_requests9/0은
실행했으며 one reply·WouldBlock·거절/실패 원자성 범위다. 명령은 같은 app lib
테스트에서 filter `retained_source_requests`이고 로그는
`/tmp/beatkernel-coreaudio-common-requests-14033.log`다.

solo/local은 같은 complete owner와 공통 Source pump를 소비하도록 연결했다.
공유 cold config는 기존 origin/gate를 명시적으로 받으며 retained Mixer 성장
한계를 초기 nominal에 고정하지 않는다. 실제 PCM·Source history·creation pin을
보존하는 64→128 성장, ceiling/interpretation 거절, scalar network gate·finite
bound, 실제 RuntimeGroup의 독립 member/capture를 host fixture7개로 실행했다.
명령은 `cargo test -p beatkernel-bms-runtime --bin macos_bms --no-default-features --locked --offline -j2 retained_source_output_fixtures`,
로그는 `/tmp/beatkernel-coreaudio-consumers-host-14033.log`다. 일반 소프트웨어
storage 성장과 native cold geometry 승인은 별도이며, 새128 렌더 전에 원래 tail을
admit하는 검사를 실제 native reopen 성공으로 해석하지 않는다. cfg-Mac consumer
port2개는 함수 포인터 조합의 형 검사다. app lib/tests/bin의 최신 cached macOS
typing은 exit0이며 `ac007-ac008-macos-startup-error.log`에 남겼다.

사전 coverage·contract·performance advisory에서 retained cohort 초기 open 실패의
all-member 마무리 누락과, 명시적 matrix의 converter scratch가 실제 callback 대신
성장 ceiling을 사용하는 문제를 확인했다. 기존 공통 finalizer로 original boxed
error와 pending owner를 보존하고, CoreAudio complete cold preparation에서만 실제
maximum에 맞춘 converter storage를 준비하도록 수정했다. 기존 public
NativeOutputState 준비와 WASAPI/ASIO 의미는 유지한다. 같은 해석의 equal-rate
converter storage 교체는 lookahead가 없고 내부 상대 counter가 외부 시간 근거가
아니므로 원래 Mixer·Source 시간, pending report/admitted prefix·PCM을 보존한다.

native registration의 create/null-ID/listener 실패는 등록 이후부터 시작하던 fixture의
공백이라 기존 static operation 주입으로 실제 install sequence fixture4개를 보강했다.
solo/local의 실제 paused UI forwarding은 구체 CoreAudio stream과 IOHID 소유자가
필요해 host 행동 실행면이 없으며, 공통 Owner/UI 테스트나 위 포트 형 검사가 그
실행을 대신하지 않는다. 실제 Mac 연결 여정과 WBS .15.12에 이 검증을 남긴다.
이 consumer 구현 시점에는 전체 task의 독립 formal code/security review·CLI QA를 아직 진행하지 않았다.

후속 검증 candidate는 solo/local9e68bbb4와 cold storage/등록 회귀32c5b78d다.
수정 후 host platform 전체473/0/ignored2와 consumer8/0을 실행했다. platform473에는
기존469와 새 actual-maximum4개가 포함된다. 최초 새 fixture의 retirement 기대1건은
연속 interval을 쪼갠다고 잘못 가정해472/1이었다. 실제 active/pause/resume 전이를
생성한 뒤 두 complete interval의 retirement revision·원래 identity를 확인하고,
대기 중 새 voice의 정확한 nonzero PCM을 성장·pause·축소 후 확인하도록 고쳤다.
로그는 `/tmp/beatkernel-coreaudio-platform-fixed-14034.log`와
`/tmp/beatkernel-coreaudio-platform-fixed-retry1-14034.log`다. 추가 consumer1개는
실제 공통 finalizer가 close/save 오류 조합에서도 모든 member의 failed-session
capture 저장을 시도하고 원래 boxed error의 allocation identity를 유지하며 초기
실패에 completion archive를 만들지 않음을 확인한다. 실제 native open 분기를 host로
실행한 테스트는 아니다. 최신 consumer 로그는
`/tmp/beatkernel-coreaudio-consumers-host-14034.log`다.

native registration4개를 포함한 native audio52개·presentation 신규6개와 앱
lib/tests/macOS bin의 cached macOS C-stub typing은 exit0(21.35초)이다. 이 명령은
platform과 app 두 package를 함께 선택해 platform cfg-test도 검사했다. 로그는
`/tmp/beatkernel-coreaudio-macos-registration-14034.log`다. 공통 source_control_owner9개와
실제 native_owned_output_fixtures9개도 host에서 실행했다. 파일명 owned_pumps를
filter로 사용한 첫 명령은0개를 선택했으므로 근거로 세지 않으며, 실제 module명으로
수정해9/0을 확인했다. 로그는 `/tmp/beatkernel-coreaudio-source-owner-14034.log`와
`/tmp/beatkernel-coreaudio-owned-pumps-corrected-14034.log`다. 위 focused 수는 전체
library와 중복될 수 있어 합산하지 않는다. 수정 Rust30개 format·diff와 문서
local 링크144개·WBS 도구50개도 확인했다. .15.08/.09는 W→V이며 D135/489는 유지한다.

최종 검토 전 source 범위 Clippy(`--lib --bins`, core/platform/app, desktop·webtransport)는
exit0(14.96초)이지만 기존 경고가 남아 strict-clean은 아니다. `--all-targets`는
변경하지 않은 `app/src/component_motion_scene_fixtures.rs:88`의 의도적 잘못된
범위 `2..1`을 기본 deny lint가 거절해 exit101이다. 이 파일은 기준48e95988과
동일함을 byte/hash로 대조했다. 전체 대상 lint PASS나 기준 커밋 전체 lint 재실행을
주장하지 않는다. 로그는 `/tmp/beatkernel-coreaudio-clippy-fixed-14034.log`와
`/tmp/beatkernel-coreaudio-clippy-source-14034.log`다. 첫 비권한 Clippy/CLI build는
기존 root 소유 cache의 fingerprint/depfile 쓰기에서 거절돼, 소유권을 바꾸지 않고
같은 cached command만 sudo로 재실행했다. 도구나 의존성은 설치하지 않았다.

공개 `macos_bms --help`와 core `audio --fixture`를 실제 build/run해 exit0이다.
후자는 실제 offline Mixer의8frame PCM·명령3개·partition invariance를 확인하며
native 재생 증거가 아니다. 로그는 `/tmp/beatkernel-coreaudio-cli-help-fixed-14034.log`와
`/tmp/beatkernel-coreaudio-runtime-fixture-14034.log`다. 이 단계의 smoke/개발 검사는
이후 independent formal review와 QA를 대신하지 않는다.

독립 DEEP code review는 frozen2c14803a에 FAIL2를 반환했고, 별도 security review는
PASS했다. 두 탐색 입력을 검증한 후 독립 정식 reviewer가 전체 변경을 재검토해
정상적인 future CoreAudio 출력 예정 시각을 Source 교체 실패로 처리하는 경로와,
최종 Pending probe 직후 승인된 history 요청의 RecoveryUnavailable이 게임 오류로
전파되는 경합을 확인했다. QA는 아직 시작하지 않았다.

같은 독립 계획 검토자가 필요한 AC012 수정안을 PASS했고 owning MCP로 PLAN에
반영했다. 기존 non-generic waiting mode를 바꾸는 대신 교체 전용 관측 port와
실제 complete 출력이 소유하는 고정2개 typed packet buffer를 사용한다. shared
pure helper는 application에 두어 domain→ports 의존성을 만들지 않는다. 실제 출력의
epoch/basis·요청 Source identity·HOST domain을 기대값으로 동결하고 첫/미래/full
packet을 모두 검증한다. native E에 Clone/Copy를 요구하지 않고 원본을 이동한다.
full[A,B]+freshC+releaseA 뒤 동일C도 공간이 생기면 보존해야 하며, 새 future 관측이
기존 두 앵커를 계속 밀어내지 않게 한다. callback/configuration/packet 오류와
stopped/authorized 상태를 매번 먼저 검사하고, 기존 controller의 timeout/cancel/
stale/frozen/generation/two-anchor publication을 유지한다. 분류는 Native의 정확한
RecoveryUnavailable 하나만 기존 authorization 재확인에 보낸다. 사전 REQ/ADR은
소스 수정 전에 갱신했고, shared source·backend와 독립 fixture 작성자를 병렬 배정했다.
최종 검토·QA·실제 native acceptance 완료를 아직 주장하지 않는다.

2026-10-15 AC012 구현과 회귀를 동결했다. 실제 출력이 보관하는 원래 두 관측은
호출자가 전달한 HOST 시각이 도달했을 때만 꺼내며, 새 관측은 대기열이 가득 차도
검사한다. 전달한 프레임과 검증한 최신 publication을 구분해 full[A,B] 상태에서
버린 C가 공간이 생긴 뒤 동일하게 다시 읽혀도 대기열에 들어갈 수 있다. 기존
Remixed adapter는 교체 전용 호출의 output·identity·now를 그대로 전달한다.
RecoveryUnavailable만 기존 Owner의 승인 상태 재확인에 보내며 다른 typed/native
오류를 숨기지 않는다. 공통 helper7·실제 controller 미래 관측6·Owner IO 경합3·
Remix3인 새 host 회귀19개를 포함해 전체 app library3279/0/ignored8을 실행했다.
최초 실행의 누락 import2건을 고쳤고, 다음3278/1의 실패는 malformed packet을
미래 시각 전부터 거절하는 올바른 동작을 테스트가 Pending으로 기대한 오류였다.
잘못된 packet은 즉시 거절하고 다른 frozen/report 위반도 각각 실제 오류 이유와
epoch/owner 보존을 검사하도록 보강한 최종 실행이다. 로그는
`/tmp/beatkernel-coreaudio-ac012-app-lib-final-14035.log`다.

최종 macOS platform/app lib/tests/macOS bin C-stub 타입 검사도 exit0이다.
native backend의 narrow classifier 회귀3개를 추가해 관련 cfg-Mac backend fixture는
24개이며 실제 실행 PASS가 아니다. native audio52·추가 presentation6·UI7과 함께
같은 검사에서 형을 확인했다. 로그는
`/tmp/beatkernel-coreaudio-ac012-macos-final-14035.log`다. 변경 Rust40개 scoped
format과 diff 검사를 통과했다. backend는 W에서 V로 돌아가며 D135/489는
유지한다. 이 기록은 재검토 전 개발 검증이며 두 번째 DEEP 탐색·정식 code/security
review·독립 QA와 실제 native acceptance는 아직 남아 있다.

## Known ceiling

Known ceiling: nonempty pending PCM은 새 fixed buffer로 완전히 나눌 수 있어야 하며,
variable destination은 empty suffix가 필요하다. 실제 callback보다 짧은 suffix를
padding하거나 다시 렌더링하지 않는다. 다양한 실제 callback에 걸쳐 여러 원본
블록을 이어 붙여야 하는 요구가 확인되면 별도의 complete 다중 블록 모델로 확장한다.
이 제한은 교체 실패를 명시하며 조용한 싱크 변경을 허용하지 않는다.
