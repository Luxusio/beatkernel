# Retained WASAPI exclusive Source 교체 — 구현 중

기존 shared complete Source Owner를 같은 mode family의 exclusive 출력으로 확장한다.
원래 Mixer/PCM/history/command producer·Runtime·입력/capture와 attempt를 보존하며
실제 native Counter/frequency를 사용한다. 새 출력 상태 머신이나 fallback을 추가하지
않는다. [Source 계약](../runtime/REQ__retained-source-output-replacement.md)과
[WBS .13](../kernel/REQ__bms-player-work-breakdown.md)의 원래 전체 범위를 따른다.

Exclusive 이벤트 출력은 실제 native buffer 전체를 한 번에 acquire/release한다.
복구된 PCM이 새 buffer의 정수 배수이면 여러 lease로 그대로 제출하며 재생성하거나
패딩하지 않는다. Exact 설정에서 미리 아는 비배수는 native 호출 전에 거절하고,
default/명시적 rounding으로 최종 geometry가 미정이면 실제 GetBufferSize 뒤 history·
reconfigure·MMCSS·prefill 전에 확인한다. 비배수의 원본 complete owner는 보존한다.
[Microsoft GetBuffer](https://learn.microsoft.com/en-us/windows/desktop/api/audioclient/nf-audioclient-iaudiorenderclient-getbuffer)와
[GetCurrentPadding](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-getcurrentpadding)의
exclusive event-buffer 조건을 따른다.

Portable worker와 Windows consumer를 분리된 source/test 쌍으로 작성하고 동결했다.
고정된 portable API의 실제 검사 뒤 native complete 연결의 source/test 쌍도 작성하고
동결했다. 독립 code review/CLI/desktop QA와 actual Windows 실행은 아직 남는다.
이전 shared 검사를 exclusive PASS로 확장하지 않는다.

## 실제 개발 검사

공통 production worker37 PASS/실패0(기존18·신규19), sizing22와 외부 worker
binary100 PASS다. 외부 binary는 Source 포함 중복 fixture와 실제 calibrated
allocation 검사1개를 포함하므로 다른 suite와 합산하지 않는다. 실제 System
allocator의 alloc/realloc/dealloc을 보정한 뒤 새 geometry/full lease·whole-multiple
복구·실패·conditional 처리에서 heap 호출0을 확인했다. 원본 PCM/report/prefix/history와
서로 다른 두 전체 lease의 실제 데이터, acquire/release/clock 오류를 검사했다.
사전 독립 coverage 점검에서 빠진 shared cold-helper 우회 분기를 보강했다.
실제 pending6/buffer4·EnginePeriod2조합/DeviceDefault1조합의 helper 호출 뒤
전체 owner 보존을 검사했다. 로그는 `portable-worker-final.log`와
`portable-allocator-final.log`이며 앞선36/99는 당시의 검사 기록이다.

Windows bin79 PASS/실패0에는 explicit blank device의 실제 회귀2개가 포함된다.
기존 parser가 명시적으로 비운 device를 이전 endpoint로 잘못 계승해 RED2를
재현했다. 공통 파서의 모순된 guard만 제거한 뒤 GREEN2와 전체79를 확인했다.
device 생략은 원래 endpoint 유지, 명시적 blank는 실제 OS default 조회, 명시적
nonempty는 exact endpoint다. actual default가 없으면 기존 오류를 보존한다.

로그는 ignored `target/wf/retained-source-exclusive-output-replacement/foundations/`의
`portable-worker-focused.log`, `portable-allocator-sizing.log`,
`explicit-default-device-actual-red.log`, `windows-bin-green.log`에 있다.
재현 명령은 platform `--lib wasapi_source_worker`, `--test audio_format
--test wasapi_source_worker_allocations`와 app `--bin windows_bms --no-default-features`
검사이며 기존 cached env/host CC/AR·locked·jobs2를 쓴다.

Windows platform/app의 lib/tests를 함께 C-stub Rust 형 검사해 exit0을 확인했다.
새 native fixture9개는 실제 Windows 실행 없이 typing만 확인했다. 실제 size guard는
GetBufferSize와 native buffer/period 일치 검사 뒤, history/reconfigure/MMCSS/prefill
전에 있다. Ready prefill과 running event는 같은 실제 mode selector를 사용한다.
이 순서의 실제 COM/resource/wake 실행은 native QA 범위다.

기존 실제 common pump4개를 포함한 Source Owner/UI37 PASS와 Source/Target
publication 외부 할당 각각1 PASS, 변경 Rust12개 scoped format/diff도 통과했다.
로그는 같은 foundations의 `windows-combined-types.log`,
`source-owner-pumps-regression.log`, `source-target-publication-allocations.log`에 있다.

platform/app의 경고 비차단 Clippy와 실제 `windows_bms --help`도 exit0이다.
경고가 남으므로 warning-clean/strict Clippy 판정은 아니다. 로그는
`platform-clippy.log`, `windows-bin-clippy.log`, `windows-public-help-smoke.log`다.
일반 health dry-run의 별도 진단값은0.0/10이었다. 위 명시적 환경에서 실제 실행한
scoped 검사나 제품 전체 검증의 대체물로 사용하지 않는다.

WBS .13의 원래 문장을 유지하고4개 실행 단위로 나눠 활성220→223이다.
새 완료는 없으며 E68 현재 D105·V73·W20·N2·E15·P5·U3,105/223(47.09%)다.
원래 제품 요구91/193(47.15%)과 다른 backend/형식/browser/물리/출시 요구를 유지한다.

## Known ceiling

Known ceiling: 실제 exclusive native 동작은 아직 증명되지 않음 — 연결 구현과 독립 native QA에서 확인 필요

Known ceiling: 실제 exclusive 재생은 AC-004 native worker와 통합·Windows 검증 전까지 미검증 — 해당 구현과 검증이 완료되면 갱신

Known ceiling: 실제 Windows 장치 동작은 미검증 — 실행 가능한 native QA 환경에서 확인 필요

위 구현 레인의 연결과 형 검사는 E68에서 확인했고, 실제 native 실행 gate는 남는다.

직전 shared native QA는 소유 Windows VM에서 Render endpoint0·로그인 콘솔과
현재 실행 환경 부재로 BLOCKED_ENV였다. 새 QA에서 가용성을 다시 확인하고
실제 exclusive 장치·clock/input·pause/replace/resume·PCM/capture/cleanup은 별도로
검증해야 한다. C-stub Rust typing은 native 실행이나 음향 증거가 아니다.
