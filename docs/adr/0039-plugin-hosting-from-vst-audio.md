# 0039. 플러그인 호스팅은 vst-audio 크레이트에 맡긴다

상태: 채택, 2026-10-22(2026-10-23 수정: vst-audio 0.2.0의 버스 선택으로 주 버스만 켜고, 0.3의 슬롯 수와 파이프라인 깊이로 디코드한 버스트의 블록을 놓치지 않습니다. 같은 날 0.4의 블록 끝점과 복구 정책으로 링 클라이언트와 앱의 감시 상태기계를 옮겼습니다. 2026-10-24 수정: 0.5의 링 순번은 2^32 일련번호라 소진되지 않습니다. 같은 날 0.6으로 올렸고 코드는 바뀌지 않습니다). ADR 0021, 0022, 0035의 크레이트 표, 링크 규칙, unsafe 예산을 바꿉니다. 프로세스 구조, 제어 채널, 상태별 시한, 링 규약, 지연, 대체 신호, 상태 저장의 결정은 그대로입니다.

## 배경

ADR 0021은 호스팅 크레이트가 `ssr-*`에 의존하지 않게 해서 나중에 따로 낼 수 있게 했습니다. 그 크레이트들(`streamring`, `plughost-proto`, `plughost-sys`, `plughost`, `plughost-testplugin`, `apps/ssr-plughost`)을 v0.20.0(b6d4df9)에서 떼어 낸 것이 [vst-audio](https://github.com/115dkk/vst-audio)이고, crates.io에 0.1.1까지 나왔습니다. 그 뒤 vst-audio에는 macOS와 Linux 호스팅, 악기와 MIDI, f64, 여러 버스, 편집기 요청, 독립 오픈소스 플러그인 시험이 더해졌고, 소프트리시버의 호스팅 크레이트는 그동안 바뀌지 않았습니다.

같은 코드를 두 저장소에서 따로 고치면 한쪽에서 찾은 플러그인 호환성 문제나 실패 처리 개선이 다른 쪽에 가지 않습니다. 소프트리시버가 크레이트를 쓰면, 소프트리시버에 필요한 호스팅 개선은 크레이트를 고쳐야만 들어오므로 크레이트가 늘 먼저 받습니다.

## 결정

### 무엇을 쓰는가

소프트리시버는 crates.io의 vst-audio를 쓰고, 판은 `Cargo.lock` 하나가 정합니다. vst-audio는 자기 크레이트끼리 `=`로 묶으므로 앱, 엔진, 워커가 늘 같은 판을 씁니다. 그래서 앱이 띄우는 워커와 앱의 규약 판이 어긋나지 않습니다.

| 크레이트 | 쓰는 곳 | 쓰는 것 |
| --- | --- | --- |
| `vst-audio` | 앱(Windows) | `Host`, `Instance`, `LoadTask`, `BlockEndpoint`, `RecoveryPolicy::Watch`. 호스트를 띄우고 블록을 처리하며 복구합니다 |
| `vst-audio-protocol` | 앱 | 메시지와 버스 배치입니다 |
| `vst-audio-ring` | 엔진 | 공유 메모리 링과 `BlockLane`. `streamring`에서 나와 f64, 이벤트, 트랜스포트, 레인마다 슬롯 수를 더한 배치 4입니다(0.5부터 슬롯 수는 2, 4, 8, 16). 엔진의 `RingUpmixer`가 쓰고, 시험이 가짜 호스트로 링 경로를 확인합니다 |
| `vst-audio-worker-runtime` | `apps/ssr-plughost` | 워커 실행 루프. `ssr-plughost.exe`는 이 크레이트의 `host::main`만 부르는 실행 파일입니다 |

- **블록 단위 고수준 API를 씁니다.** vst-audio 0.4의 `BlockEndpoint`는 호출자의 블록 k를 싣고 정한 깊이 m만큼 앞의 블록 k−m을 돌려주며, 누락 때 대체 신호를 만들지 않습니다. 따라서 업믹스 단계의 계약(`D = (m + 1)·B + L + C`, ADR 0035)과 맞습니다. 앱은 `Host::spawn_async`의 `LoadTask`를 폴링하고 `Instance::take_blocks()`로 끝점을 받습니다.
- **`streamring`은 남습니다.** ASIO 입구(ADR 0025)가 씁니다. 두 링은 배치 버전(1과 3)이 달라 한 구역에서 섞이지 않습니다.
- **주 버스만 호스팅합니다.** `plughost`는 첫 입력 버스와 첫 출력 버스만 켰습니다. vst-audio 0.1.1의 `PluginProcess::start`는 플러그인이 선언한 버스를 모두 켜서, 사이드체인이 업믹서의 입력 스피커에 섞이고(모노 사이드체인은 C 스피커가 되어 장면이 그리로 접힘) 보조 출력이 출력 스피커에 섞였습니다. 그래서 처음에는 앱 어댑터가 켜진 버스가 한 방향에 둘 이상인 호스트를 거절했고, 사이드체인이나 보조 출력을 가진 업믹서는 쓸 수 없었습니다. 이것이 아래 "호스팅을 고치는 순서"의 첫 사례였습니다. vst-audio 0.2.0이 적재와 준비 사이에 버스를 고르는 `BusSelector`를 더했고, 0.4 어댑터는 `Configuration::buses`에 `BusSelector::main_only()`(방향마다 첫 주 버스만 켬)를 넣습니다. 사이드체인과 보조 출력은 꺼진 채로 남고 그런 업믹서도 쓸 수 있습니다. 거절(`only_main_buses`)은 꺼진 버스를 세지 않으므로, 플러그인이 받은 것보다 많은 버스를 보고하는 경우를 막는 검사로 남습니다.
- **링은 슬롯 여덟 개를 씁니다.** 앱 어댑터는 `Configuration::slots`로 `UPMIX_RING_SLOTS`(8)를 넘겨 호스트를 띄우고, `Instance::take_blocks()`로 받은 `BlockEndpoint`를 엔진의 `Upmixer`로 감싸 넘깁니다(`BlockUpmixer`). 단계가 세대를 시작하며 `set_depth(m)`을 부르면 끝점이 `restart(m)`로 블록 번호와 파이프라인을 다시 시작하고, 그 앞에 실은 블록은 돌려주지 않습니다. 순번은 이어집니다. 끝점은 k−m의 결과를 읽은 뒤 블록 k를 게시하므로 m은 슬롯 수보다 작아야 합니다. m은 소스에 따라 48 kHz에서 PCM 1, DD+·AC-3·JOC 4, TrueHD·MAT 3입니다(ADR 0035의 2026-10-23 고침). 엔진의 `RingUpmixer`는 같은 계약을 `vst_audio_ring::BlockLane`으로 지키는 링 클라이언트로 남아, 슬롯이 여덟보다 적은 링을 거절하고 엔진 시험에서 가짜 호스트와 함께 쓰입니다.
- **크레이트 넷이 빠집니다.** `plughost`, `plughost-proto`, `plughost-sys`, `plughost-testplugin`(약 7,100줄)입니다. 실행 파일 이름 `ssr-plughost.exe`는 그대로여서 설치본, 앱의 경로, 시험서명 PC 안내는 바뀌지 않습니다.

### 호스팅을 고치는 순서

- 워커, 프로세스와 Job 객체, 파이프, 링, 시한과 실패 판단, VST 2와 VST 3 어댑터, 편집기, 상태 저장을 고칠 일은 vst-audio 저장소에서 고칩니다. vst-audio는 main에 머지할 때마다 crates.io에 냅니다. 그 판이 나오면 소프트리시버가 `Cargo.lock`의 판을 올립니다.
- 소프트리시버는 `[patch]`나 git 의존성으로 호스팅을 덮어쓰지 않습니다. `deny.toml`의 출처 목록도 그대로입니다.
- 두 저장소를 함께 고치는 동안에는 커밋하지 않는 cargo 설정으로 옆의 체크아웃을 붙입니다. 소프트리시버가 직접 쓰는 넷을 모두 붙여야 링 타입이 한 벌이 됩니다.

  ```sh
  V=../vst-audio/crates
  cargo test --workspace \
    --config "patch.crates-io.vst-audio.path='$V/vst-audio'" \
    --config "patch.crates-io.vst-audio-protocol.path='$V/vst-audio-protocol'" \
    --config "patch.crates-io.vst-audio-ring.path='$V/vst-audio-ring'" \
    --config "patch.crates-io.vst-audio-worker-runtime.path='$V/vst-audio-worker-runtime'"
  ```

  붙이는 동안 cargo가 `Cargo.lock`을 경로 출처로 고쳐 쓰므로, 커밋하기 전에 `git checkout Cargo.lock`으로 되돌립니다. `ssr-policy`의 외부 시험 확인은 레지스트리 출처만 받으므로 붙인 동안에는 실패합니다. 그 두 시험은 판을 올린 뒤 돌립니다.

### 시험

- 호스팅의 시험은 vst-audio에 있고, 옛 `plughost` 시험과 이름이 같습니다. `features.toml`은 `Cargo.lock`이 잠근 vst-audio 크레이트의 시험을 가리킬 수 있고, `ssr-policy`가 `cargo metadata`로 그 판의 소스를 찾아 시험이 있는지 확인합니다.
- `ci.yml`의 `plughost` 잡은 잠긴 판의 vst-audio 태그를 받아 시험용 플러그인(`vst-audio-testplugin`)을 빌드하고, vst-audio의 `native` 시험을 소프트리시버가 싣는 `ssr-plughost.exe`로 돌립니다. 러너는 전과 같이 Windows Server 2022, 2025, Windows 11 ARM64이고, x64에서는 암호화한 제3자 업믹서도 적재합니다. vst-audio의 CI는 Windows에서 Server 2025 x64만 돌리므로, 나머지 둘과 제3자 업믹서는 이 잡이 맡습니다.
- 엔진의 업믹스 시험(가짜 호스트 위의 `RingUpmixer`, 무할당)은 `vst-audio-ring`으로 그대로 돕니다. 시험 호스트를 호출하는 동안 멈춰 두고, 480프레임 PCM(깊이 1)과 1536프레임 2.0 E-AC-3 버스트(깊이 4)가 블록을 놓치지 않는지, 버스트 둘을 한 번에 놓으면 깊이를 넘는 블록만 놓치고 다시 돌아오는지도 확인합니다.

### 정책 게이트와 unsafe

- `unsafe-budget.toml`에서 `plughost-sys`(181)와 `plughost-testplugin`(126)이 빠집니다. 격리 크레이트는 `ssr-sys`, `ssr-setup-sys`, `asio-sys` 셋이고, Windows API 의존성을 가질 수 있는 첫 파티 크레이트도 이 셋입니다.
- vst-audio의 unsafe는 그 저장소의 `-sys` 크레이트(전송, 워커 쪽 ABI와 편집기 창)에 있고, 그 저장소의 규칙(`-sys`에만, 블록마다 안전성 설명)과 CI(ASan, 퍼징)가 다룹니다.
- 링크 규칙은 `ssr-policy`가 검사합니다.
  - 남의 플러그인을 적재하는 `vst-audio-worker-runtime`과 `vst-audio-worker-sys`는 `apps/ssr-plughost`만 링크합니다. 앱 프로세스에 플러그인 로더가 들어오지 않습니다(ADR 0021).
  - `vst-audio`와 `vst-audio-transport-sys`는 앱만 링크합니다. 엔진은 `vst-audio-ring`만 씁니다.
  - `streamring`과 `ssr-plughost`는 `ssr-*`에 의존하지 않습니다.

### 라이선스

vst-audio는 MIT입니다. 워커 쪽 `vst-audio-worker-sys`는 Xaymar vst2sdk(BSD-3-Clause)를 옮긴 파일과 DPF(ISC)에서 대조한 배치를 담아 `MIT AND BSD-3-Clause AND ISC`이고, `deny.toml`이 이미 허용합니다. 출처 기록은 vst-audio의 `NOTICE`에 있습니다.

## 결과

- 소프트리시버에서 크레이트 넷과 손으로 쓴 unsafe 307이 빠집니다.
- 호스팅 개선은 vst-audio를 거쳐야만 소프트리시버에 들어옵니다. 판을 올리는 PR이 하나 더 들고, vst-audio의 릴리스가 끝나야 받을 수 있습니다.
- 소프트리시버 CI는 vst-audio의 태그, 시험 이름, 환경 변수(`VST_AUDIO_*`)에 묶입니다. 판을 올릴 때 그 시험이 바뀌었으면 잡도 함께 고칩니다.

## 2026-10-23 결정: 블록 끝점과 복구를 0.4에 맡긴다

vst-audio 0.4가 필요한 블록 단위 API와 `RecoveryPolicy::Watch`를 갖추었으므로 다음 경계를 채택합니다.

- **vst-audio로 옮긴 것:** `RingUpmixer`가 중복해 갖던 링 클라이언트(블록 번호, 순번, 게시와 읽기, 파이프라인, 내부 세대)는 `vst_audio_ring::BlockLane`이 맡습니다. Windows 앱 어댑터가 갖던 0.5초, 2초, 8초 재시작 정책과 10분 실패 창, 복구할 최신/이전 상태의 선택, 새 워커를 실시간 끝점에 넘기는 복구 인계는 `RecoveryPolicy::Watch`, `Instance`와 `BlockEndpoint`가 맡습니다.
- **소프트리시버에 남긴 것:** 엔진은 장면을 플러그인 입력 배치로 접고, 같은 시각의 대체 신호와 은폐를 만들며, 소스 전달 크기로 깊이 m과 전체 지연 D를 정합니다. 앱은 두 상태 파일을 영속화하고, 소스 레이트나 플러그인 지연이 달라질 때 예전 인스턴스에 `SaveState`와 `Shutdown`을 요청해 마지막 상태를 받은 뒤 새 레이트로 띄우는 인계를 맡습니다.
- 앱은 `HostEvent`를 감독자 사건으로 옮기되 `NeedsRespawn`은 내지 않습니다. 실패와 복구는 대체 신호 상태로, 복구 불가는 비활성화로, 달라진 지연은 감독자가 위의 레이트/지연 인계를 시작하도록 알립니다.

아직 정하지 않은 것은 CI 소유권입니다. vst-audio의 CI에 Windows Server 2022와 Windows 11 ARM64를 더하고 제3자 업믹서의 비밀을 그 저장소에 둘 때까지, 이 저장소의 `plughost` 잡은 그 환경과 판을 계속 확인합니다.

## 2026-10-24: 0.5로 올린다

vst-audio 0.5.0은 링 순번을 2^32 일련번호로 다뤄, 블록 750개/초에서 약 66일 뒤 `BlockLane`이 발행을 멈추고 업믹스가 대체 신호로만 남던 순번 소진을 없앴습니다(vst-audio ADR 0005). 링 배치는 4이고 슬롯 수는 2, 4, 8, 16만 받습니다. 엔진의 `UPMIX_RING_SLOTS`(8)는 그대로 맞으므로 코드는 바뀌지 않습니다. 링이 업믹서보다 작으면 거절하는 시험만, 0.5가 링 자체를 거절하는 7 대신 4슬롯 링을 씁니다. ASIO 입구의 `streamring`도 같은 규칙으로 고쳤습니다(ADR 0040).

## 2026-10-24: 0.6으로 올린다

vst-audio 0.6.0은 unsafe 구간 감사에서 나온 결함을 고친 판입니다(vst-audio#9). minor로 올라간 까닭은 `vst-audio-worker-runtime`의 `PluginEvents`가 worker-sys의 타입을 재수출하게 바뀌어서인데, 이 타입은 `Plugin` 트레이트를 구현하는 쪽만 씁니다. `ssr-plughost.exe`는 `host::main`만 부르고, 앱과 엔진이 쓰는 `vst-audio`, `vst-audio-protocol`, `vst-audio-ring`의 시그니처는 그대로여서 코드는 바뀌지 않습니다. 링 배치(4)와 제어 규약도 그대로입니다.

고친 것 가운데 Windows 제품에 닿는 것은 워커 쪽 둘입니다.

- VST 3 워커는 메인 스레드의 `setState`나 비활성화가 admission을 닫는 순간 오디오 스레드가 처리에 들어가 있으면, 그것을 플러그인 결함으로 보고하고 끝났습니다. `RecoveryPolicy::Watch`는 이것을 충돌로 세었습니다. 0.6은 `setProcessing`과 `process`가 실제로 실패했을 때만 결함으로 보고하고, 닫힌 admission은 블록을 미완료로 두어 엔진이 대체 신호를 냅니다.
- 워커가 샌드박스를 걸 때 토큰 그룹 목록을 원소 하나짜리 배열 참조에서 늘려 읽던 정의되지 않은 동작을 고쳤습니다.

공유 매핑 봉인, 이벤트 소켓, 디스크립터 상속, 프로세스 그룹 종료의 수정은 Linux와 macOS의 전송 계층이라 이 제품에는 해당하지 않습니다. `features.toml`이 가리키는 vst-audio 시험의 이름은 0.6에서도 그대로입니다.
