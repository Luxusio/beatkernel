# 고밀도 채보의 완전한 표시

## 요구사항

BeatKernel 기반 BMS 플레이어는 준비 단계에서 승인한 채보를 표시할 때,
동시에 보이는 노트나 지뢰가 2,048개를 넘는다는 이유만으로 거절하거나
일부를 생략하지 않는다. 이 요구는 native와 browser의 공통 표시 경로에
적용한다. 판정, 입력, 오디오 시계는 표시 용량과 독립적으로 유지한다.

채보의 기존 `MAX_SOURCE_ITEMS` 승인 한도를 유지한다. 노트, 지뢰, BGA,
opacity 이벤트를 합산해야 하며 각각 별도 최대치를 허용하지 않는다.
표시용 인스턴스의 보수적 상한은 `3 * notes + mines`다. 탭은 head 하나,
hold는 body/tail/head, head가 소비된 hold는 body/tail, 지뢰는 하나를
표시한다. 판정이 완료된 노트는 표시에서 제외한다. 승인된 가시 객체의
순서와 형상은 기존 규칙을 따른다.

## 저장 공간과 장치 한도

가시 노트는 기존 시간 인덱스로 조회한다. 지뢰는 정렬된 시간 범위로
조회한다. 작은 채보에도 전체 소스 한도만큼 scratch나 GPU 버퍼를 미리
확보하지 않는다. 필요한 저장 공간을 확보하고, 반복 사용 시 용량을
유지한다. 가시 객체와 표시 조건이 유지되는 동안 geometry Arc와 GPU
업로드 식별자를 재사용하고, 시간 이동은 기존 local epoch 방식으로
처리한다. seek, resize, progress 또는 가시 객체가 달라지면 형상을
다시 구성할 수 있다. 전체 프레임의 무할당을 보장하는 요구는 아니다.

GPU 어댑터는 실제 요청한 device의 `max_buffer_size`와 checked draw
count/byte 범위를 확인한다. 버퍼가 부족할 때만 확장한다. 표시량이 조금씩
증가할 때 매번 버퍼를 교체하지 않도록 기하급수적으로 용량을 늘리되,
소스에서 유도한 최대 인스턴스 byte 수와 실제 장치 한도를 넘지 않는다.
필요한 byte 수가 두 한도 안에 있으면 여유 용량 때문에 거절하지 않는다.
교체한 버퍼에는 같은 geometry Arc라도 데이터를 다시 올려야 한다.
한 프레임의 모든 playfield를 검사한 후 surface 획득, queue write,
업로드 식별자 변경과 성공 presentation 처리를 시작한다.

숫자 범위, 승인 용량 또는 실제 장치 한도를 넘으면 구체적인 표시 오류를
반환한다. 일부 playfield만 정상적으로 제출한 것처럼 표시하지 않는다.
실패한 Scene 후보는 clear 또는 새 후보를 구성하기 전까지 성공적으로
제출할 수 없다. 오류 화면은 별도의 정상 후보로 구성한다. CPU 저장 공간의
복구 가능한 reservation 실패는 이전 cache를 교체하지 않고 반환한다.
global allocator, Arc 생성, GPU 드라이버의 모든 OOM을 복구할 수 있다는
보장은 하지 않는다. 장치 오류와 surface 복구는 기존 경로를 따른다.

## Browser와 레이어 분리

browser wire는 기존처럼 전체 채보와 progress를 전달하며 geometry를
전송하지 않는다. wire 형식 변경은 이 요구에 필요하지 않다. 동시에
표시하는 최대 네 playfield와 최대 64명 roster의 progress 보관은 별개다.
CPU/GPU peak 계산은 scratch, cache metadata, 현재 및 교체 geometry,
renderer가 보유한 이전 Arc와 버퍼 교체 중 공간을 보수적으로 반영한다.
공유된 동일 allocation은 중복 합산하지 않는다.

scratch, cache와 renderer의 저장 공간은 이전 채보에서 남을 수 있으므로
현재 채보 크기만으로 보관 공간의 상한을 계산하지 않는다. 네 표시 슬롯의
소스 최대치에서 유도한 보수적 추정치를 사용한다. 이는 실제로 그만큼
할당하거나 사용한다는 뜻이 아니다.

`peak_visual_transport_bytes`는 메모리 추정 helper이며 런타임의 전체
메모리 승인 정책이 아니다. 기존 packet/source/image/diagnostic 승인
규칙을 유지한다. 표시 준비 성공이나 GPU 제출 성공이 오디오 재생 준비를
대체하지 않는다. [로딩 준비 계약](REQ__loading-readiness.md)을 함께 따른다.

## 검증과 현재 상태

의도한 동작의 계약이며 구현과 검증은 진행 중이다. 이 문서 자체는
현재 구현 완료의 증거가 아니다. 실제로 동시에 2,048개를 초과하는
tap/hold/mine fixture와 독립적인 ordered membership/primitive oracle로
누락을 검사한다. 8,192개 이상의 인스턴스로 기존 GPU 용량을 넘어서는
native/browser 제출을 실행한다. 장치 한도 직전/직후, 늦은 playfield의
실패, clear 이후 복구, warmed 저장 공간 및 Arc 재사용을 확인한다.
스크린샷만으로 모든 인스턴스의 존재를 증명하지 않는다.

Known ceiling: software GPU에서의 실제 제출은 물리 장치의 성능이나
입력/오디오 지연을 증명하지 않는다. 실제 장치 수용 검사는 별도다.

Known ceiling: 공개 notes를 같은 길이로 임의 수정하면 기존 prepared
index의 의미가 달라질 수 있다. 그런 변경은 기존 준비 경로에서 다시
준비해야 한다. 매 프레임 전체 채보 해싱이나 immutable API 전환은
이 작업에 포함하지 않는다.

Known ceiling: peak helper는 요청한 payload/layout 저장 공간을 추정한다.
allocator의 용량 반올림, 드라이버와 queue의 내부 보관, surface와 정적 UI
cache, 비시각 상태를 포함한 실제 resident RAM 상한은 보장하지 않는다.
입력 cold registration의 교체가 겹치면 이전 registration의 cold 저장
공간을 따로 더해야 한다. 표시 cache와 scratch는 이전 채보의 보관 공간을
소스 최대치로 반영한다.
