# 실제 플레이의 고정 양수 배속

상태: 구현·검증 시작. 완료나 현재 지원을 선언하는 문서가 아니다.
전체 플레이어 Goal과 WBS05.13의 나머지 요구는 유지한다.

실행 중 전환의 별도 계약은 [runtime 배속](REQ__runtime-playback-rate.md)이다.
해당 후속 기능은 구현 중이며 이 문서의 고정 배속 검증을 전환 완료로 계산하지 않는다.

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

분수 자체가 유효해도 cold 초기 곡 시각
`S + trunc(-P*N/D)`가 signed 64-bit 범위를 벗어나면 시작을 거절한다.
브라우저 Window는 실제 section start snapshot으로 오디오·입력 소유자를
만들기 전에 검사하며 speed 필드에 focus와 복구 가능한 오류를 표시한다.
Worker 직접 요청도 파일 읽기·준비 작업 전에 같은 공통 수치 계약을 검사한다.
네이티브는 장치 조회·open 이전에 공통 `LiveSpeedSetup`으로 검사한다.
양수 분수의 허용 범위를 임의로 줄이지 않고 실제 start/preroll 조합을 판단한다.
명시적 finite end의 역배속 출력 시간이 native zero-origin timestamp 범위를
넘는 경우도 파일·장치 open 이전에 거절한다. 알려진 sample Hz는 실제 grid로
검사하고, Windows의 backend 협상 전에는 공통 1ns grid로 확정적인 overflow를
먼저 검사한 뒤 실제 협상 Hz의 rounding을 별도로 검사한다.

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

네이티브 준비 함수도 실제 배속을 header·capture·saved comparison·네트워크
identity에 전달한 뒤 외부 파일·endpoint를 획득한다. 공통 native_judge의
`prepare_policy_header_with_speed`, `prepare_policy_competition_identity_with_speed`,
`prepare_section_capture_for_policy_with_speed`는 기존 인자 뒤에 `ReplaySpeed`를
받는다. 경쟁의 `prepare_native_section_with_policy_and_speed`와
`prepare_member_section_with_policy_and_speed`, group의
`prepare_with_policies_and_speed`도 같은 선택을 받는다. Cohort의
`prepare_audio_cohort_with_policy_and_speed`는 불변 `LiveSpeedSetup`과 실제
section 설정이 일치하는지 확인하고 모든 member에 같은 조건을 전달한다.
기존 함수는 NORMAL의 기존 비교·기록·네트워크 바이트를 유지한다.
기본 BeatKernel 정책의 역사적인 NORMAL 비교 경로도 보존한다. 명시적으로
선택한 nonnormal 배속은 실제 policy capture의 전체 조건을 비교 기준으로
사용하여 동일 배속·section·정책의 실제 자기 기록을 받을 수 있어야 한다.
새 배속 envelope를 과거 header에만 덧붙여 실제 생성 기록과 달라지게 하지 않는다.

## 유한 출력 이후의 실제 명령 정산

Native 솔로와 로컬 cohort도 이전에 검증한 출력 보고서를 보존하고, 실제
종료 표식을 관찰한 뒤에는 다음 보고서의 정산 보존 관계와 고정 상태를
검사한다. 최종 완료 직전 재조회에도 같은 검사를 적용하며, 중간 `None`
조회가 이전 표식을 잊게 해서는 안 된다. 단일 보고서의 합계가 맞더라도
종료 후 applied 증가, expired 감소, 원곡 위치·voice 변경 등 불가능한
진행은 완료 근거로 채택하지 않는다.

브라우저 실행 검증은 실제 Worklet terminal 진단을 매 cleanup 이후와 최종
확인에서 검사한다. 종료 정리 중 발생한 실제 출력 오류도 실패로 기록하며,
정상 Worker 완료나 깨끗한 console/network만으로 덮지 않는다.

출력 보고서를 읽은 직후 마지막 callback이 실행되어, 그 다음 원곡 입력이
유효한 소리 명령을 발행하는 순서도 포용한다. 실제 유한 endpoint 이후의
비어 있지 않은 callback은 기존 pending과 이번 큐 snapshot을 하나의
`max_commands_per_render` 예산 안에서 소비한다. 유효하지만 재생할 출력이
없는 명령은 `commands_expired`로 기록하고 `commands_applied`로 세지 않는다.
일반 pause·startup·empty render·unlimited 출력에는 적용하지 않는다.
잘못된 gain/sample/rate/time는 기존 오류 카운터로 보고하며 만료로 숨기지 않는다.
callback은 기다리거나 할당하거나 producer의 새 명령을 끝없이 따라가지 않는다.

PCM은 무음, playback cursor·endpoint·voice·rate·song 상태는 고정한다.
완료는 실제 consumed=admitted와 checked applied+expired=admitted를 요구하며
presentation·입력 drain·BGM credit 조건도 유지한다. 포화·overflow는 증거가 아니다.
이 정산을 관찰하기 전에는 출력 callback을 중단하지 않는다.
실제 endpoint 이후 보고서는 consumed·expired의 단조 증가와 pending 감소만
허용하고 `Δexpired = Δconsumed + 이전 pending − 현재 pending`을 검증한다.
원곡 입력·판정·gauge·capture와 raw `audio_at`은 유지한다.

공개 Step의 cold command admission은 immutable endpoint 이상으로 예약된
target을 거절한다. 이 경로에서 만들 수 없는 terminal pending을 가짜 ACK로
검증하지 않는다. 실제 늦은 queue 발행은 Step·native owner에서 검사하고,
독립 producer가 만든 실제 pending의 감소는 같은 공유 보고서 검증기에서
검사한다. 유한 replay의 `output_finished()` 조회도 실제 정산 전에는 기다리며
정산 이후에도 원래 source/converted target presentation을 요구한다.

예상 HOST 시작점 뒤에서 수집한 입력도 실제 오디오 관측으로 매핑하면
logical audio origin 이전일 수 있다. 기존 시작 전 입력 폐기 정책을 이
경우에도 명시적으로 적용한다. frozen input descriptor의 실제 매핑을 기준으로
판별하고, Runtime·practice 위치·키 상태에 전달하기 전에 큐에서 소비한다.
브라우저 touch 위치·page routing 계산보다 먼저 분류하여 버린 입력의 위치나
source routing이 다음 정상 입력에 적용되지 않게 한다.
별도 discard commit은 원본 입력 chronology와 revision만 갱신하며 실제
operation·presentation·song·판정·capture·오디오 명령을 만들지 않는다.
origin과 같은 입력 및 이후 입력은 정상 처리하고 preroll의 음수 song time은
허용한다. 입력을 0으로 clamp하거나 HOST 시계로 재생을 진행하지 않는다.
정상 input commit은 시작 전 descriptor를 거절하고, stale·domain·원본 순서·
허용된 extrapolation 검사는 그대로 유지한다. Step·native solo/cohort가 같은
분류를 사용하며 브라우저는 실제 폐기 수를 기존 수집 단계 폐기 수와 합산한다.

`AudioCounters`에는 명시적 `commands_expired` 필드가 추가된다. 전체 필드를
직접 쓰는 Rust struct literal은 이 필드를 추가하거나 `..Default::default()`를
사용한다. Native telemetry는 기존 38개 scalar 인덱스 뒤에 이 값을 붙인다.
Browser output report는 기존 인덱스 0–27을 유지하고 expired를 28에 붙인
29개 u64 / 58개 u32 / 232바이트다. 실제 BrowserAudio의 `report_fields=29`를
Worklet이 확인하며, 이전 provider·56-word 보고서·다른 버전의 bindings/WASM은
거절한다. 관련 자산은 함께 재생성·배포하고 padding이나 기본값으로 호환을 꾸미지 않는다.

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
cleanup과 stale generation fencing을 따른다. live 재생 중에는 replay 배속
label과 hint를 실제 레이아웃에서도 숨기고, replay 재생 중에는 live 배속
control을 숨긴다. hidden 속성뿐 아니라 실제 표시 크기와 화면으로 검증한다.
브라우저 live/replay 배속 draft는
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

Retained native 배속은 별도 [활성 구현 계약](REQ__retained-playback-speed.md)에
따라 기존 출력 소유자에 연결한다. 이 링크는 구현·QA 완료를 뜻하지 않는다.
