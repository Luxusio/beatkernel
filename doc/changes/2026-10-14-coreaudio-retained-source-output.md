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
backend/UI/solo/local 연결, 독립 코드/보안 검토, 전체 CLI QA와 최종 문서 감사는
남는다. 이 기반 테스트 통과가 전체 retained 교체의 완료를 뜻하지 않는다.

사용자가 WBS 세분화를 요청해 미완료117개를383개 실행 단위로 나눴다. 기존
완료106개·조건부4개·원래91/193과 모든 원래 범위를 보존했고, 기존 실제 검사
근거가 있는 하위29개를 드러내 새 기준선은135/489(27.61%), 남음354개다.
분해 전106/223(47.53%)과 직접적인 진척 증감 비교에 사용하지 않는다.
CoreAudio .15는12개로 분해했으며 portable 기반3개 D, native 소유권/fixture2개 V,
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

## Known ceiling

Known ceiling: nonempty pending PCM은 새 fixed buffer로 완전히 나눌 수 있어야 하며,
variable destination은 empty suffix가 필요하다. 실제 callback보다 짧은 suffix를
padding하거나 다시 렌더링하지 않는다. 다양한 실제 callback에 걸쳐 여러 원본
블록을 이어 붙여야 하는 요구가 확인되면 별도의 complete 다중 블록 모델로 확장한다.
이 제한은 교체 실패를 명시하며 조용한 싱크 변경을 허용하지 않는다.
