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

이는 실행 중 배속 전환 전체의 완료가 아니다. 현재 AC001–004와 AC013의 기반
검증을 마쳤다. AC012 오디오 WASM의 첫 check는 cold sample_limit accessor 누락으로
실패했고 누락을 추가한 뒤 browser-audio lib check가 통과했다. scoped format은
기존 파일별2021/2024 style을 유지해 통과했다.
공통 owner와 native/browser UI 수직 연결, 실제
WASM/화면 동작, 전체 독립 code/security review와 필수 QA는 남아 있다.
WBS05.13.06은 W, 완료 수는136/489다. 기존 Chromium strict network와 실제 장치
acceptance의 환경 한계도 유지한다. Native IO06은 입력 항목도 포함하므로
그 전체0/32를 순수 오디오 진행률로 합산하지 않는다.

실제 native launcher6곳과 공통 audio/startup/policy가 여전히 fixed constructor를
사용하는 연결 누락도 확인했다. AC014를 같은 기능의 후속 조립 단위로 명시하고
AC010 통합 검사의 선행으로 추가했다. 새로운 제품 요구나 WBS 완료 수 증가는
아니다. 현재 AC005 owner와 AC007 UI의 구현·독립 테스트 작성 두 쌍이 병렬 진행
중이며, 작성 중 source에 대한 빌드는 두 쌍의 writer가 멈춘 뒤 수행한다.
