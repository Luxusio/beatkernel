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

현재 production CoreAudio callback과 native/backend/UI/solo/local 연결을 진행
중이다. 독립 코드/보안 검토, CLI QA와 최종 문서 감사는 아직 남아 있다.
이 기반 테스트 통과가 전체 retained 교체의 완료를 뜻하지 않는다. WBS .15는
N에서 W로 옮겼으며 완료 수106/223·원래91/193과 분모를 유지한다.

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
