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

현재는 실제 source 기반 독립 계획 검토와 계약 기록 단계다. portable worker와
Windows consumer를 분리된 source/test 쌍으로 작성하고 native complete 연결은
고정된 portable API 뒤에 진행한다. 실행·typing·독립 code review/CLI/desktop QA는
각각 실제 결과를 얻은 뒤 기록한다. 이전 shared 검사를 exclusive PASS로 확장하지 않는다.

WBS .13의 원래 문장을 유지하고4개 실행 단위로 나눠 활성220→223이다.
새 완료는 없으며 D105·V70·W22·N3·E15·P5·U3,105/223(47.09%)다.
원래 제품 요구91/193(47.15%)과 다른 backend/형식/browser/물리/출시 요구를 유지한다.

## Known ceiling

직전 shared native QA는 소유 Windows VM에서 Render endpoint0·로그인 콘솔과
현재 실행 환경 부재로 BLOCKED_ENV였다. 새 QA에서 가용성을 다시 확인하고
실제 exclusive 장치·clock/input·pause/replace/resume·PCM/capture/cleanup은 별도로
검증해야 한다. C-stub Rust typing은 native 실행이나 음향 증거가 아니다.
