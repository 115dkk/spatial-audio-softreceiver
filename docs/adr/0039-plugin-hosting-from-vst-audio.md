# 0039. 플러그인 호스팅은 vst-audio 크레이트에 맡긴다

상태: 채택, 2026-10-22. ADR 0021, 0022, 0035의 크레이트 표, 링크 규칙, unsafe 예산을 바꿉니다. 프로세스 구조, 제어 채널, 상태별 시한, 링 규약, 지연, 대체 신호, 상태 저장의 결정은 그대로입니다.

## 배경

ADR 0021은 호스팅 크레이트가 `ssr-*`에 의존하지 않게 해서 나중에 따로 낼 수 있게 했습니다. 그 크레이트들(`streamring`, `plughost-proto`, `plughost-sys`, `plughost`, `plughost-testplugin`, `apps/ssr-plughost`)을 v0.20.0(b6d4df9)에서 떼어 낸 것이 [vst-audio](https://github.com/115dkk/vst-audio)이고, crates.io에 0.1.1까지 나왔습니다. 그 뒤 vst-audio에는 macOS와 Linux 호스팅, 악기와 MIDI, f64, 여러 버스, 편집기 요청, 독립 오픈소스 플러그인 시험이 더해졌고, 소프트리시버의 호스팅 크레이트는 그동안 바뀌지 않았습니다.

같은 코드를 두 저장소에서 따로 고치면 한쪽에서 찾은 플러그인 호환성 문제나 실패 처리 개선이 다른 쪽에 가지 않습니다. 소프트리시버가 크레이트를 쓰면, 소프트리시버에 필요한 호스팅 개선은 크레이트를 고쳐야만 들어오므로 크레이트가 늘 먼저 받습니다.

## 결정

### 무엇을 쓰는가

소프트리시버는 crates.io의 vst-audio를 쓰고, 판은 `Cargo.lock` 하나가 정합니다. vst-audio는 자기 크레이트끼리 `=`로 묶으므로 앱, 엔진, 워커가 늘 같은 판을 씁니다. 그래서 앱이 띄우는 워커와 앱의 규약 판이 어긋나지 않습니다.

| 크레이트 | 쓰는 곳 | 쓰는 것 |
| --- | --- | --- |
| `vst-audio` | 앱(Windows) | `PluginProcess`, `PluginControl`, `watch::ControlWatch`. 호스트를 띄우고 명령하고 감시합니다 |
| `vst-audio-protocol` | 앱 | 메시지와 `StateHistory` |
| `vst-audio-ring` | 엔진 | `RingUpmixer`의 공유 메모리 링. `streamring`의 배치 2(f64, 이벤트, 트랜스포트를 더함)입니다 |
| `vst-audio-worker-runtime` | `apps/ssr-plughost` | 워커 실행 루프. `ssr-plughost.exe`는 이 크레이트의 `host::main`만 부르는 실행 파일입니다 |

- **저수준 API를 씁니다.** vst-audio의 `Host`, `Instance`, `AudioEndpoint`는 호출자의 블록을 자기 양자로 다시 묶느라 양자 둘만큼 늦고, 대체 신호는 입력 채널 i를 출력 채널 i로 보냅니다. 업믹스 단계는 블록 k를 싣고 k−1을 받는 계약(D = 2B + L + C, ADR 0035)과, 입력 배치를 출력 배치로 패닝하는 대체 신호를 씁니다. 둘이 맞지 않으므로 엔진의 `RingUpmixer`와 앱 어댑터(재시작, 감시 넘기기, 상태 파일 둘)는 소프트리시버에 남습니다.
- **`streamring`은 남습니다.** ASIO 입구(ADR 0025)가 씁니다. 두 링은 배치 버전(1과 2)이 달라 한 구역에서 섞이지 않습니다.
- **주 버스만 호스팅합니다.** `plughost`는 첫 입력 버스와 첫 출력 버스만 켰습니다. vst-audio 0.1.1의 `PluginProcess::start`는 플러그인이 선언한 버스를 모두 켭니다. 그대로 두면 사이드체인이 업믹서의 입력 스피커에 섞이고(모노 사이드체인은 C 스피커가 되어 장면이 그리로 접힘), 보조 출력이 출력 스피커에 섞입니다. 버스 목록은 시작 도중의 `Loaded`에서야 알 수 있어서 `start_configured`에 미리 넘길 수도 없습니다. 그래서 앱 어댑터는 켜진 버스가 한 방향에 둘 이상인 호스트를 이유와 함께 거절합니다(`only_main_buses`). 사이드체인이나 보조 출력을 가진 업믹서는 vst-audio가 시작할 때 주 버스만 켜는 선택을 갖출 때까지 쓸 수 없습니다. 이것이 아래 "호스팅을 고치는 순서"의 첫 사례입니다. 그 판이 나오면 어댑터가 그 선택으로 시작하고, 거절은 비활성 버스를 세지 않으므로 그대로 둡니다.
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
- 엔진의 업믹스 시험(가짜 호스트 위의 `RingUpmixer`, 무할당)은 `vst-audio-ring`으로 그대로 돕니다.

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

## 다음에 정할 것

- vst-audio의 고수준 API가 양자 하나만 늦는 블록 단위 끝점과 `ControlWatch`의 정책(0.5초, 2초, 8초 뒤 재시작, 10분 안에 세 번이면 끄기)을 갖추면, 앱 어댑터의 재시작 상태기계와 `RingUpmixer`의 링 쪽도 vst-audio로 옮길 수 있습니다. 그러면 소프트리시버에는 장면 접기와 대체 신호만 남습니다.
- vst-audio의 CI에 Windows Server 2022와 Windows 11 ARM64를 더하고 제3자 업믹서의 비밀을 그 저장소에 두면, 이 저장소의 `plughost` 잡은 판을 확인하는 데만 남습니다.
