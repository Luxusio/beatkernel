# 실제 플레이의 고정 양수 배속

상태: 구현·검증 시작. 완료나 현재 지원을 선언하는 문서가 아니다.
전체 플레이어 Goal과 WBS05.13의 나머지 요구는 유지한다.

## 시간과 소리

선택 배속은 양수 ASCII `NUM/DEN`이며 기본값은1/1이다. 분자는 i64 양수,
분모는 u64 양수 범위에서 정규화한다. 소리의 read-head/resampling 속도와
원본 곡 시간에 같은 배속을 적용하므로 pitch도 바뀐다. 출력 sample Hz,
buffer, preroll, lookahead와 입력 수집 HOST 시각은 배속으로 바꾸지 않는다.

출력 활성 경과 E, 출력 preroll P, 원본 시작 S와 배속 N/D에서 곡 시간은
`S + trunc((E-P)*N/D)`다. signed 전체식을 한 번 나누고 최종 timestamp만
좁힌다. `trunc(-P*N/D) + trunc(E*N/D)`로 나누어 계산하지 않는다.
P=1ns,N/D=1/2,E=2ns일 때 결과는 S이고 S+1ns가 아니다.
검증된 finite end는 양수 초과를 narrowing 전에 제한할 수 있지만 음수
overflow나 검증되지 않은 end를 숨길 수 없다.

물리 입력은 기존 InputMerger/AudioAuthority로 원래 논리 오디오 좌표에
정규화한 후 하나의 Runtime Transport에서 원곡 시간으로 변환한다.
원본 acquisition timestamp, source sequence와 original_clock_point를 유지한다.
키음·hazard는 기존 `report.audio_at`의 원시 출력 시각에 예약하며,
해당 시각에 원곡 배속을 다시 곱하지 않는다. BGM은 원곡 start를 제거하고
역배속 D/N으로 출력 시각에 옮긴 뒤 출력 preroll을 한 번 더한다.
배경음이 없어도 입력 키음 전에 실제 origin SetRate가 있어야 한다.
실제 zero-frame finite fence에서는 명령을 억지로 추가하지 않는다.

## Transport와 일시정지

기존 Transport::new 및 integer-anchor/seek/rate 변경 계약은 유지한다.
새 cold opt-in `Transport::new_with_preroll(origin,start,preroll,rate,end)`는
별도 setup 오류를 반환하며 원래 start, 출력 preroll, rate, finite end와
실제 활성 출력 경과의 위상을 하나의 Transport history에 보존한다.
정수 anchor snapshot만으로 exact cold setup을 검증하지 않는다.
전체 불변 setup을 조회하거나 비교할 수 있어야 한다.

실제 NativePause boundary와 prepared authority cutoff를 검증한 뒤
`pause_at_output_elapsed(cutoff,active_elapsed_ns)`로 위상을 고정하고,
`resume_preroll(cutoff)`로 동일 위상에서 재개한다. opt-in 경로는 기존
pause 뒤 정수 seek로 위상을 버리지 않는다. 기본 legacy pause/resume는
기존 계약을 유지한다. explicit seek와 실제 nonzero rate 변경은 원래 정수
양자화 계약으로 전환하며, no-op rate는 위상을 보존한다.

P=1ns,1/2,활성 E=2ns에서 pause한 곡 시간은 S다. 재개 뒤 활성 E=3ns는
S+1ns여야 한다. P=3ns,3/2에서 E=1ns의 S-3ns를 고정한 뒤 E=2ns는
S-1ns가 되어야 한다. pause gap은 출력 시간이며 기존 source/converted
target/interval cutoff, resume floor, chronology와 clone→검증→commit을 유지한다.

공통 live pause의 opt-in API는 `update_live_pause_with_speed` 및
`update_live_audio_pause_with_speed`에서 `LiveSpeedSetup`을 받는다.
Transport 준비는 `prepare_live_transport_with_speed` 및
`prepare_live_audio_transport_with_speed`에서 실제 `sample_rate`를 마지막
인자로 받아 boundary의 `playback_frame`을 활성 출력 경과로 변환한다.
정수 `boundary.song`만으로 분수 위상을 복원하지 않는다. 기존 prepare
함수들은 legacy 계약을 유지하고 배속 소비자는 명시적인 opt-in 함수를 쓴다.

## 기록과 경쟁 조건

녹화는 원곡 operation/song_time과 원본 물리 provenance를 유지한다.
플레이 당시 source speed는 정규화한 optional versioned setup envelope로
보존한다. NORMAL은 envelope 없이 기존 바이트를 그대로 유지한다.
순서는 speed→timing→judgments→gauge→base로 고정한다. 명시적 NORMAL
envelope, 미정규 분수, 중복 nesting, unknown version, 잘린 길이, trailing
bytes와 header budget 초과는 거절한다. Source speed를 표현하지 못하는
legacy tuple decoder는 nonnormal metadata를 조용히 버리지 않고 거절한다.

외부 speed envelope는 `bms-speed-setup/v1:` 뒤에 inner options 길이
(`u32` little-endian), 양의 numerator (`i64` little-endian), denominator
(`u64` little-endian), 정확히 해당 길이의 inner options를 담는다.
`RecordedSetup.source_speed`는 envelope가 없으면 NORMAL이다.
`setup_play_policy_header_with_speed`와
`LiveReplayCapture::new_with_policy_and_speed`는 기존 policy 인자 뒤에
source speed를 받으며 기존 생성자들은 NORMAL 바이트를 유지한다.
명시적인 play policy가 없는 소비자는 `new_with_gauge_and_speed`에서
기존 `new_with_gauge` 인자 뒤에 source speed를 전달한다. 기존 judge/gauge를
그대로 보존하며 배속을 기록하기 위해 새 판정 정책을 만들어 넣지 않는다.

원곡 판정 복원에는 기록 speed를 다시 곱하지 않는다. 선택한 리플레이
출력 배속은 기록 당시 조건과 별개다. 경쟁·saved opponent는 기존 canonical
setup 비교에 실제 source speed를 포함하여 동일 조건만 비교한다.
native record catalog는 현재 선택한 실제 배속을 사용한다.

## 끝·UI·오류

네이티브 solo는 `run_gameplay_audio_with_result_and_ports_with_speed`와
`run_gameplay_audio_with_policy_and_result_and_score_and_ports_with_speed`,
cohort는 `run_cohort_audio_with_ports_and_speed`와
`run_cohort_audio_with_ports_and_policies_and_speed`로 시작한다.
기존 인자 뒤에 불변 `LiveSpeedSetup`을 전달하며 기존 API 기본 동작은 보존한다.
잘못된 setup은 기존 `NativeGameplayResult` 오류로 native 제어 효과 전에 거절한다.

브라우저 solo의 `new_and_speed`, `new_section_and_speed`,
`new_physical_and_speed`, `new_physical_contact_and_speed`와 local의
`new_physical_and_speed`는 기존 인자 뒤에 fraction 문자열을 받는다.
정규화한 `speed_numerator`와 `speed_denominator`는 BigInt getter로 확인한다.
기존 Prepared의 실제 play policy를 공통 정책 생성자로 전달하며,
기존 생성자는 원래 기본 경로를 유지한다.

정수 매핑이 original end에 도달했다는 것만으로 출력 종료를 승인하지 않는다.
실제 admitted/consumed/applied, BGM retirement/ACK와 원래 source presentation
또는 converted target crossing이 확보됐을 때만 마지막 원곡 Advance(end)로
solo/cohort 각 member의 judge/gauge/capture frontier를 닫는다.
출력 프레임 양자화로 원곡 end보다 정수 위치가 짧으면, 위 종료 권한을
확인한 뒤 실제 마지막 논리 operation 시점에서 같은 Transport를 명시적으로
end에 seek하고 기존 Runtime advance를 실행할 수 있다. 이는 terminal
원곡 종료 처리이며 새 시계나 실행 중 seek가 아니다. 선택한 불변 setup과
기록 배속은 보존하고 이후 입력·resume를 허용하지 않는다.
zero/quantized endpoint도 같은 권한이 필요하다. Renderahead, wall cutoff,
미완료 SetRate/Stop 또는 source-only converted 관측은 종료 근거가 아니다.

실제 immutable finite source 출력 종료 표식이 검증된 뒤에는 해당 Runtime의
게임플레이 오디오 발행을 닫는다. 종료 전 시각의 유효 입력이 뒤늦게 도착해도
원래 판정·hazard·gauge·provenance·기록을 처리하고 `audio_at`을 보존한다.
이미 소리를 낼 수 없는 출력에 새 키음·입력음·hazard음 또는 실패 Stop을 넣지
않으며, 기존에 발행한 명령·credit·ACK는 그대로 검증한다. 임시 pause나 숫자상
곡 종료는 이 경계를 닫지 않는다. 변환 출력의 source가 먼저 닫혀도 target
presentation 통과와 입력 drain 없이는 플레이 완료로 인정하지 않는다.

native --speed와 기존 Settings 필드 및 browser live 배속 필드는 NUM/DEN,
1/1 기본값과 pitch 변화 의미를 표시한다. 잘못된 값은 파일·장치·오디오
시작 전에 거절하고 UI에서 focus를 유지한다. Preparing/Playing은 선택 snapshot을
잠그고 실제 owner의 정규화한 identity와 대조한다. Stop/retry는 기존 once
cleanup과 stale generation fencing을 따른다. 브라우저 live/replay 배속 draft는
구분하며 per-frame DOM이나 VDOM을 도입하지 않는다. native profile은 기존
flag-pair 형식을 사용하고 empty/default 바이트는 유지한다.

Worker live 요청의 `liveSpeed`는 fraction 문자열이며 생략하면 `1/1`이다.
`replaySpeed`는 별도의 replay 선택 값이다. Preparing 응답은 정규화된
`liveSpeed`와 BigInt `liveSpeedNumerator`/`liveSpeedDenominator`를 반환하며,
기존 `endNs`/`endFrame`의 단위와 종료 권한은 유지한다. 첫 비동기 대기 전에
선택값을 복사하고 실제 WASM getter와 프레임 경계를 대조한다.

## 검증과 남은 범위

리터럴 네 배속, 홀수 preroll/반복 pause, MIN/MAX/typed refusal, 실제 Mixer
PCM와 queue credit, original capture round trip 및 실제 current WASM/Worker/
Worklet/renderer로 검사한다. 실제 native Settings 메뉴와 CLI도 검사한다.
현재 이 문서는 요구 계약이고 검증 PASS를 주장하지 않는다.

Known ceiling: 실행 중 배속 변경, retained practice/loop/seek/restart, reverse와
물리 음향·외국 OS 실행은 후속 전체 Goal 요구다. 지원하지 않는 nonnormal
retained 경로는 cold 단계에서 명확히 거절하며 조용히1x로 실행하지 않는다.
