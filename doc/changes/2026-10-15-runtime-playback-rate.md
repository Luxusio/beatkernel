# 실행 중 배속 전환의 공통 위상 기반

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

새 Clippy 표현식 경고는 수정한 뒤19개를 다시 확인했고 touched Rust format과
diff 검사는 통과했다. strict Clippy는 기존 audio/practice.rs의 prepare와
audio/source_playback_history.rs의 preflight 두 곳에 있는 too_many_arguments로
실패했다. 두 signature가 기반 HEAD에 이미 있음을 확인했고 경고를 숨기지 않았다.

이는 실행 중 배속 전환 전체의 완료가 아니다. 실제 Mixer control/PCM, Transport
history 연결, 가변 source recording, native/browser owner/UI와 전체 독립 review/QA는
아직 남아 있다. WBS05.13.06은 N에서 W로만 옮겼고 완료 수는136/489다.
기존 Chromium strict network와 실제 장치 acceptance의 환경 한계도 유지한다.
