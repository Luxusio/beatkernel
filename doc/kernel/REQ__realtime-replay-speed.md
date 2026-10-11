# 실시간 리플레이의 고정 유리수 배속

상태: 고정 양수 리플레이 배속 구현 및 독립 code/security/CLI/browser/UX 검증 완료.
최종 문서 확인·Harness close와 전체 플레이어 완료는 별도다.
원래 [배속 요구](../../plan.md)의 고정 양수 pitch-changing 부분을 실제
공통·브라우저·네이티브 리플레이에 연결한다. [오프라인 배속](REQ__replay-render-playback-speed.md)을 재사용한다.

## 선택과 호환성

- 기존 생성자는 1/1로 동작한다. 선택한 배속은 세션 동안 불변이다.
- `ReplaySpeed`는 양수로 정규화한 core `Rate`를 보관한다. 엄격한 ASCII
  `NUM/DEN` 문법이며 분자는 1..i64::MAX, 분모는 1..u64::MAX다.
- 네이티브 `play_replay_bms --speed NUM/DEN`은 파일·장치 접근 전에 검증한다.
  `--rate`와 `--output-rate`는 계속 PCM Hz, `--seconds`는 출력 실행 시간이다.
- 브라우저는 Replay speed (NUM/DEN) 필드로 선택한다. 기본 1/1, 최대 40자,
  pitch 변화 안내와 label을 제공한다. 준비·재생 중에는 잠근다.
  오디오 시작 전에 유효성을 검사하고 정규화한 문자열을 세션에 고정한다.
- Worker는 준비 전에 검증하며 실제 WASM owner의 정규화한 배속과 응답을
  대조한다. 비정상 배속을 지원하지 않는 패키지를 1배속으로 대체하지 않는다.
  live 요청에는 비정상 배속을 적용하지 않는다.

`StepReplay::new_with_rate(..., config, Rate)`와 `playback_rate()`는 기존
config 모양과 normal 생성자를 유지한다. `ReplayPause::new_with_rate`는
sample Hz와 playback Rate를 별도 인자로 받는다.
`BrowserReplay::new_with_speed(prepared,preroll_ns,fraction)`는 consuming
static 생성자다. 실패해도 이동된 prepared 소유권은 기존 생성자와 같다.
`speed_numerator`/`speed_denominator`는 실제 정규화한 배속의 BigInt property다.
Worker 요청의 `replaySpeed`는 선택한 분수 문자열이며 생략하면 1/1이다.
리플레이 준비 응답은 `replaySpeed`, `replaySpeedNumerator`,
`replaySpeedDenominator`로 정규화한 문자열과 BigInt 구성요소를 전달한다.
Host는 시작 시 고정한 선택과 응답을 대조한다. 기존 1배속 패키지의 생성자·
배속 getter 부재는 1배속 호환 경로에서만 허용한다.
브라우저 숫자 모델 `replaySpeedFromFraction`은 immutable snapshot을 반환하고,
`replayOutputFromMetadata`의 기존 네 인자 호출은 계속 1배속·100ms preroll이다.

## 곡 시간과 출력 시간

출력 경과 E, 출력 preroll P, 원본 시작 S, 배속 N/D에 대해 곡 시간은
`S + trunc((E - P) * N / D)`다. 일시정지로 생긴 실제 출력 gap은 E에서
먼저 제거한다. P와 gap을 곡 시간으로 오해하지 않는다. 실제 출력 presentation만
곡 진행을 승인하며 렌더 선행이나 호스트 polling은 승인 근거가 아니다.

기존 NativePause의 누적 gap 정책을 재사용한다. 곡 origin을 i64::MAX로
설정한 조회 결과에서 그 anchor를 wide 정수로 빼면, 음수 출력 origin이
허용하는 전체 signed physical span에서도 중간 negative gap narrowing으로
유효한 곡 시간을 잘못 거절하지 않는다. 새 시계나 pause 정책을 도입하지 않는다.
변환 출력의 누적 TargetTime도 0 기준의 signed duration으로 먼저 좁히지
않는다. 표현 가능한 음수 anchor에 더한 실제 TargetTime 결과에서 wide
duration을 복원한 뒤, 공급된 곡 anchor에서 빼고 최종 값만 좁힌다.
기존의 누적 duration nanosecond floor는 한 번만 적용한다. 결과 자체가
표현 불가능하면 기존 typed 오류로 거절하며 owner 상태를 채택하지 않는다.
동일 NativePause의 target presentation-origin 매핑도 gap과 유효한
playback-origin/startup 차이를 wide로 합친 후 최종 값만 좁힌다. 중간
`original - gap`이 표현 불가능하더라도 최종 상쇄 결과가 표현 가능하면
거절하지 않는다. 기존 domain·startup·presentation 순서 검증은 유지한다.

계산은 checked wide 정수로 수행한다. 검증된 원본 finite end가 있으면 wide
곡 값을 먼저 cap하고 Timestamp로 한 번만 좁힌다. 비유한 overflow는 오류다.
native source·converted target·assessed interval과 fallback 모두 같은 규칙이다.
`ReplayPause::with_song_end`는 성공한 request/observe 상태 채택 전의 cold
설정이며 end는 원본 시작 이상이다. 순수 `presentation_song(&self)` 조회는
관측 이력을 채택하지 않으므로 setup을 봉인하지 않는다. 이 조회에 interior
mutation을 추가하여 기존 Sync 등 AutoTraits를 바꾸지 않는다.
실패한 관측·매핑은 owner를 부분적으로 갱신하지 않는다.

명령 시간과 finite output frame은 기존 rate-aware planner가 결정한다.
원본 assets를 두 번 crop하지 않는다. origin SetRate도 실제 queue credit을
사용하며 Stop allowance를 늘리지 않는다. 원본 녹화 bytes·프로필·seed·section·
판정 시간·judge hash는 배속으로 변경하지 않는다.

기존 `BgmFeeder::new`와 `from_output_commands`의 Play/Stop 검증은 유지한다.
배속 리플레이는 `from_output_commands_with_rate(commands,config,Rate)`로
명시적으로 선택한다. 양수 배속이며 추가 preroll은 0이어야 한다. 1배속은
기존 명령만 받는다. 비정상 배속의 비어 있지 않은 plan은 ordinal 0에
정확히 한 개의 matching origin SetRate를 포함해야 한다. 다른 배속·시각·
중복·지연 제어 명령과 Seek는 거절한다. 0-frame fence의 빈 plan은 허용한다.
명령은 원래 bounded queue/retirement 경로를 사용한다. live 호출의 권한을
확장하거나 제어 명령의 credit을 무시하지 않는다.

## 유한 구간 종료

반올림으로 원본 end 직전에서 곡 시간이 멈출 수 있다. 실제 core endpoint,
정확한 admitted/consumed/applied 수, feeder remaining/outstanding=0 및 실제
presentation을 확인했을 때만 남은 원본 records를 end까지 진행한다.
0-frame fence도 같은 원칙을 따른다. 최소 출력 frame을 임의로 추가하지 않는다.

`FiniteReplayCompletion::output_finished`는 records 완료와 독립적으로 이
출력 근거를 조회한다. source owner는 실제 physical marker presentation,
converted owner는 기존 NativeEnd의 실제 target crossing을 요구한다.
기존 observe 종료 판단에는 실제 records_finished 조건을 그대로 유지한다.
native는 Running에서만 이 근거로 원본 end를 진행하며 `--seconds`에서도
적용한다. 가짜 records_finished나 source-only converted 종료는 금지한다.

## 검증과 남은 범위

독립적으로 작성한 literal 1/2·1·2·3/2 시간·PCM 기대값, 실제 Mixer와
producer/ACK/feeder, 반복 pause 및 original source/target authority를 검사한다.
기본 API 회귀, malformed/max/overflow/cap/zero fence/실제 queue 거절도 검사한다.
브라우저는 현재 생성한 WASM·Worker·Worklet과 실제 UI 재생으로 검증한다.
mock 요청 테스트만으로 실제 재생 완료를 주장하지 않는다.

Known ceiling: live 배속, 실행 중 배속 변경, retained practice·역재생·pitch
보존 DSP 및 물리 장치의 음향·성능 검증은 이 고정 배속 child의 완료 범위 밖이다.
원래 전체 Goal과 WBS05.13/05.14 및 조건부 DSP 요구에 그대로 남는다.
실제 검증 범위·명령·첫 실패와 재검사·환경 한계는
[변경 근거](../changes/2026-10-12-realtime-replay-speed.md)에 기록한다.
네이티브 null startup은 자연 종료나 음향 정확도의 증거가 아니다.
