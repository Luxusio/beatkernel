# 진행 중 작업을 포함한 계층형 WBS

기존 제품 WBS의07.08 출력 교체와07.09 출력 설정 연결을 각각10개와5개 실행
단위로 나눴다. 구현된 기반 코드, 미완료 controller, 아직 구현하지 않은 Owner/UI와
공통 pump, Source/다른 플랫폼/실제 장치 검증을 같은 목록에서 추적한다.
원래 두 요구사항의 ID·완료 기준·선행 조건은 유지했다.

## 실제 집계

기준 HEAD는 `1463ccfa`다. 아래 수치는 `tools/wbs_status.py --json`을 실제 실행한
결과이며 테스트 작성자의 예상이나 제품 개발 완료율이 아니다.

| 집계 기준 | 변경 전 | 변경 후 |
| --- | --- | --- |
| 원래 활성 제품 요구사항 | 193 | 193 |
| 원래 검증 완료 요구사항 | 91 / 193 (47.15%) | 91 / 193 (47.15%) |
| 활성 말단 실행 단위 | 193 | 206 |
| 완료 말단 실행 단위 | 91 / 193 (47.15%) | 97 / 206 (47.09%) |
| 남은 말단 실행 단위 | 102 | 109 |
| 조건부 말단 | 4 | 4 |
| 구조 부모 | 0 | 2 |
| 기능 ID coverage | 60 | 60 |

분모 증가는 `193 - 2 + 15 = 206`이다. 늘어난 완료6개는 이미 커밋되고 검사된
coordinate 기반4개와 Linux encoding software 범위2개를 드러낸 것이며, 이번
추적 도구 작업으로 제품 기능6개를 새로 구현한 것이 아니다. 원래 부모의 전체
완료 기준을 충족했다는 뜻도 아니다.

변경 후 말단 상태는 D97 / V64 / W20 / N3 / E14 / P5 / U3이며, D+V161은
문서상 구현 상태의 개수다. V64에는 검증·연동이 남으므로 완료97에 더하지 않는다.

## 동작과 유지 규칙

기존 보고 도구에 계층 ID와 순수 parse/report/ready/in-progress 함수를 추가했다.
기본 보고와 JSON은 말단 집계와 원래 요구사항 집계를 함께 표시한다.
`--in-progress`는 말단 W/V/N을, `--ready`는 부모에서 상속된 선행 조건까지
충족한 후보를 보여 준다. 각 단위의 근거와 다음 검사는
[정식 WBS](../kernel/REQ__bms-player-work-breakdown.md)에 있다.

부모·자식을 중복 집계하지 않는다. 누락된 직계 부모, 조건부 부모 아래 활성
자식, 부모·말단의 완료 상태 불일치, 선언·상속·부모 완료 관계의 순환은 오류로
거절한다. 상태를 자동으로 D로 올리거나 파일을 변경하지 않는다.
진행 중 구현은 안정적인 하위 ID에 연결하고 실제 구현/검증 milestone마다 상태,
범위 한정 근거와 다음 검사를 갱신한다. 과거24-AC 매핑과60개 기능 survey는
역사적 기록으로 유지하며 현재 실행 단위와 구분한다.

## 실행한 개발 검사

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools -p test_wbs_status.py -v
python3 tools/wbs_status.py
python3 tools/wbs_status.py --json
python3 tools/wbs_status.py --ready
python3 tools/wbs_status.py --in-progress
git diff --check -- tools/wbs_status.py tools/test_wbs_status.py doc/kernel/REQ__bms-player-work-breakdown.md doc/kernel/REQ__bms-player-progress.md doc/changes/2026-10-14-wbs-ongoing-work-breakdown.md
```

첫 실행은48개 PASS, 실제 subprocess 검사를 추가한 최종 실행은
**50개 PASS / 실패0**이다. flat/deep literal
집계, checkbox/state/근거, 직계 부모, 조건부 범위, 완료 상태 양방향 검사,
effective 순환, 상속된 readiness, 진행 목록, 실제 CLI 경계와 읽기 오류를 검사했다.
실제 interpreter subprocess는 정식 tools/doc 경로의 임시 fixture로 네 모드,
WBS/inventory 누락·형식 오류·모드 충돌을 실행하고 exit0/1/2와 출력·원본 보존을 확인했다.
정식 문서에 대한 네 보고 모드는 모두 exit0이다. 현재 controller는 ready에
나오며 그 결과에 의존하는 미구현 Owner/UI와 pump는 ready에 나오지 않는다.
독립 review-code 다음 qa-cli 및 runtime verify/close는 이 개발 검사와 별도 gate다.

개발 로그는 ignored `target/wf/wbs-ongoing-work-breakdown/`의
`unit-first.log`, `unit-final.log`, `default.log`, `current-counts.json`, `ready.log`,
`in-progress.log`에 있다. 기존 미커밋 native 후보6개는 `native-before.json`의
SHA256과 일치해 이 WBS 작업에서 변경되지 않았다.

## 남은 범위

현재 retained task의 AC004는 마지막 실제 실행18개 중15 PASS/3 FAIL 뒤 fixture를
수정했으나 재실행하지 않은 V다. 새 AC005 Owner/UI와 AC006 공통 pump는 N이다.
기반 검사나 ALSA null/stub 결과를 물리 소리·지연·실제 paused GUI·다른 OS/SDK·
Source 전체 완료로 확대하지 않는다. full player Goal과 원래193개 요구사항의
남은 범위는 유지한다. 제품 소스 수정·Rust 재빌드·의존성 설치·push는 이 작업에 없다.
