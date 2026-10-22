# 0022. 손으로 쓰는 unsafe를 격리 크레이트 몇 개에 나눠 둔다

상태: 채택, 2026-10-12. ADR 0002의 'unsafe는 `ssr-sys` 한 크레이트에만'과 'Windows 의존성은 `ssr-sys`만'을 고칩니다.

## 배경

ADR 0002는 손으로 쓰는 unsafe를 `ssr-sys` 하나에 모았습니다. ADR 0020, 0021, 0025로 unsafe가 필요한 곳이 셋 생깁니다. 플러그인 호스팅(VST 2 ABI, VST 3 COM, DLL 적재)과 ASIO 드라이버(IASIO COM 서버)는 반드시 unsafe를 쓰고, 퍼스트파티 업믹서는 가속이 필요할 때만 씁니다.

이것을 `ssr-sys`에 넣으면 장치와 드라이버를 다루는 크레이트가 플러그인 ABI와 업믹서 가속까지 떠안아 검토할 범위가 섞입니다. 그리고 `ssr-sys`는 `ssr-types`와 `ssr-abi`에 의존하므로, 그 안에 넣은 호스팅 코드는 HostVST로 따로 낼 수 없습니다.

## 결정

- 손으로 쓰는 unsafe를 가질 수 있는 크레이트를 격리 크레이트라고 부르고, 이름은 `-sys`로 끝냅니다. 지금은 `ssr-sys` 하나이고, 이 ADR로 `hostvst-sys`, `asio-sys`, `upmix-sys`가 자격을 얻습니다. 셋 다 아직 만들지 않았고, `upmix-sys`는 ADR 0020의 조건(벤치로 보인 필요)을 채울 때만 만듭니다.
- 격리 크레이트는 `unsafe-budget.toml`에 올라야 합니다. 처음 숫자는 크레이트를 만드는 PR에서 실제 개수로 정하고 이 ADR 끝에 덧붙입니다. 그 뒤 숫자를 올리거나 새 격리 크레이트를 더하려면 ADR이 먼저입니다.
- 격리 크레이트는 `ssr-sys`와 같은 lint 표를 씁니다. 루트는 `#![deny(unsafe_code)]`이고 unsafe를 쓰는 모듈만 허용하며, `undocumented_unsafe_blocks`, `multiple_unsafe_ops_per_block`, `missing_safety_doc`을 deny로 둡니다.
- 격리 크레이트 바로 위에는 그것을 감싸는 안전한 크레이트나 얇은 앱을 둡니다(`hostvst-sys` 위에 `hostvst`, `upmix-sys` 위에 `upmix`, `asio-sys` 위에 `apps/ssr-asio`). 그 위의 코드는 unsafe 없이 씁니다.
- Windows API 의존성(`windows`, `windows-sys`, `windows-core`, `wasapi`)은 `ssr-sys`, `hostvst-sys`, `asio-sys`만 가집니다. `upmix-sys`는 가지지 않습니다. `upmix`는 `engine`이 쓰고, `engine` 이하는 Linux에서 빌드되어야 하기 때문입니다.
- 링크 규칙은 다음과 같습니다.
  - `ssr-sys`는 지금처럼 `apps/` 아래의 앱만 의존합니다(`ssr-replay` 제외).
  - `hostvst-sys`는 `hostvst`만, `[target.'cfg(windows)'.dependencies]`로 의존합니다. `hostvst`는 앱만 의존합니다(`ssr-plughost`, M4의 `spatial-softreceiver`).
  - `upmix-sys`는 `upmix`만 의존합니다.
  - `asio-sys`는 `apps/ssr-asio`(ASIO 드라이버 DLL)만 의존합니다.
  - `engine`은 `streamring`, `hostvst-proto`, `upmix`를 의존할 수 있습니다. `streamring`은 unsafe도 Windows 의존성도 없는 보통 크레이트입니다.
  - `streamring`, `hostvst`, `hostvst-sys`, `hostvst-proto`는 `ssr-*` 크레이트에 의존하지 않습니다.
- `ssr-policy`의 Windows 의존성 게이트는 허용 목록을 갖도록 바꾸고, 위 링크 규칙과 hostvst 계열의 독립성을 검사하는 게이트를 더합니다. 게이트는 첫 새 격리 크레이트를 더하는 PR에서 함께 바꿉니다. 그전까지는 지금 게이트가 `ssr-sys`만 허용합니다.

## 결과

- unsafe를 검토할 크레이트가 하나에서 최대 넷으로 늘지만, 크레이트마다 다루는 대상(장치와 드라이버, 플러그인 ABI, ASIO 드라이버, 업믹서 가속)이 하나씩이라 검토 범위가 섞이지 않습니다.
- `CLAUDE.md`와 `CONTEXT.md`의 'unsafe는 `ssr-sys`에만'을 '격리 크레이트에만'으로 바꿉니다.
- HostVST를 떼어 낼 때 unsafe와 Windows 의존성이 `hostvst-sys` 한 곳에 있으므로 `streamring`과 hostvst 크레이트 셋을 그대로 옮기면 됩니다.

## 2026-10-15 덧붙임

ADR 0032가 격리 크레이트 `ssr-setup-sys`(관리자 작업, `apps/ssr-setup`만 링크)를 더하고 `ssr-sys`의 예산을 40까지 올렸습니다. 실제 숫자는 `unsafe-budget.toml`에 있습니다.

## 2026-10-16 덧붙임: 플러그인 호스트의 격리 크레이트 둘

ADR 0035가 `hostvst` 계열의 이름을 `plughost` 계열로 바꿨습니다. 그 첫 PR에서 격리 크레이트 둘을 더하고 숫자를 정합니다. 숫자는 `unsafe-budget.toml`의 셈법을 따릅니다. 주석·문자열을 뺀 `unsafe` 낱말 수입니다.

| 크레이트 | 예산 | 담는 것 | 링크하는 곳 |
| --- | --- | --- | --- |
| `plughost-sys` | 181 | 공유 메모리 매핑과 원자 슬라이스(한 곳), 이벤트, 잡 객체로 묶은 자식 프로세스, 익명 파이프, MMCSS, DLL 적재, 편집기 창, VST 2 ABI(Xaymar vst2sdk의 클린룸 헤더, BSD-3-Clause, 그 고지를 파일 머리에 둠), VST 3 COM(`vst3` 0.3.0, MIT 또는 Apache-2.0) | `plughost`만, `[target.'cfg(windows)'.dependencies]`로 |
| `plughost-testplugin` | 126 | 시험 전용 플러그인 DLL. VST 2 진입점과 VST 3 팩토리를 함께 내보내는 스테레오 → 5.1 업믹서이고, 상태로 고르는 죽음·멈춤·NaN 변형을 가짐 | 아무도 링크하지 않음. CI가 빌드해 `ssr-plughost`에 적재시키며, 설치본과 릴리스에 싣지 않음 |

- **VST 3이 숫자를 키웠습니다.** COM 인터페이스는 호출과 구현 양쪽에서 `unsafe` 메서드 선언이 낱말로 셈에 듭니다.
  - 시험용 플러그인의 VST 3 쪽만 104입니다. 그 가운데 51은 COM 메서드 선언이고, 블록은 50입니다.
  - 블록마다 `// SAFETY:`를 둡니다.
- **Windows 의존성 게이트를 허용 목록으로 바꿨습니다.** 허용 목록은 `ssr-sys`, `ssr-setup-sys`, `plughost-sys`입니다.
- **링크 규칙도 `ssr-policy`가 검사합니다.**
  - `plughost-sys`는 `plughost`와 시험용 플러그인만 링크합니다.
  - `plughost`는 앱만 링크합니다.
  - `streamring`과 `plughost-*`는 `ssr-*`에 의존하지 않습니다.

## 2026-10-18 덧붙임: `ssr-sys` 134

ADR 0037(74)과 0038(107)에 이어, 시험서명 PC에서만 볼 수 있던 것을 러너에서 확인하는 도구 둘이 `ssr-sys`에 27을 더합니다. 숫자는 `unsafe-budget.toml`의 셈법(주석·문자열을 뺀 `unsafe` 낱말 수)입니다.

- **엔드포인트 속성 읽기(8):** `ssr-live host formats`가 우리 엔드포인트의 속성 저장소에서 설명(`PKEY_Device_DeviceDesc`), 친숙한 이름, 장치 형식(`PKEY_AudioEngine_DeviceFormat`, VT_BLOB)을 읽습니다. `PROPVARIANT` 접근과 해제입니다. 형식 해석은 `ssr_abi::wave`에서 Linux로 시험합니다.
- **독점 비트스트림(19):** `ssr-live bitstream probe`와 `play`가 IEC 61937 형식으로 `IsFormatSupported`와 이벤트 구동 `Initialize`를 부르고, PASS에 E-AC-3 버스트를 씁니다(`IAudioClient`, `IAudioRenderClient`, `IAudioClock`, 이벤트 핸들, MMCSS). 실시간 쓰기 고리에는 할당, 잠금, 파일 입출력이 없습니다.

이 도구들은 `driver.yml`의 시험서명 설치 잡이 부릅니다(`endpoint_format_check.py`, `bitstream_check.py`, `endpoint_name_check.py`).

## 2026-10-21 덧붙임: `asio-sys`와 ASIO 입구의 숫자

ADR 0025의 첫 PR에서 격리 크레이트 `asio-sys`를 더하고 숫자를 정합니다. 셈법은 위와 같습니다.

| 크레이트 | 숫자 | 담는 것 | 링크하는 쪽 |
| --- | --- | --- | --- |
| `asio-sys` | 216 | ASIO SDK ABI(함수형 포함), in-proc COM 서버, 콜백 버퍼, Windows 전달(공유 메모리, 이벤트, 피어 핸들, MMCSS, QPC), 모듈 수명, DLL을 경로로 적재하는 시험용 probe(`probe` 기능) | `apps/ssr-asio`만 |

- **`ssr-sys`는 107에서 133이 됩니다(+26).** 앱 쪽 ASIO 호스트의 제어 블록과 링 매핑, 이벤트, 피어 핸들입니다. 바로 앞 덧붙임의 M3·M4 마무리(134)와 합치면 160입니다.
- **`ssr-setup-sys`는 52에서 57이 됩니다(+5).** ASIO 등록의 레지스트리 작업 다섯입니다(ADR 0032의 관리자 작업).
- **`asio-sys`의 라이선스는 `GPL-3.0-or-later AND GPL-3.0-only`입니다.** SDK 정의를 손으로 옮긴 `sdk.rs`가 `GPL-3.0-only`이고, 크레이트에 GPLv3 전문(`LICENSE`)을 둡니다(ADR 0025).

## 2026-10-21 덧붙임: MAT 관찰 도구, `ssr-sys` 179

Windows 공간 음향이 MAT 2.x 안에 PCM을 담는 배치는 공개 문서가 없습니다. 그래서 알고 있는 신호와 위치를 공간 음향에 넣고 드라이버 링에 들어온 MAT와 맞춰 보는 블랙박스 관찰 도구 `ssr-live mat-probe`를 둡니다(ADR 0005가 허락한 관찰). 이 도구가 `ssr-sys`에 19를 더해 160에서 179가 됩니다.

- **공간 음향 클라이언트(19):** `IMMDevice::Activate`로 `ISpatialAudioClient`를 얻습니다. `SpatialAudioObjectRenderStreamActivationParams`를 `VT_BLOB` `PROPVARIANT`로 넘겨 스트림을 열고, 이벤트마다 정적·동적 소리의 버퍼를 채웁니다. 모두 `crates/ssr-sys/src/spatial.rs` 한 파일에 있습니다.
- **나머지는 Linux에서 시험합니다.** 신호 수열, 일정, 진실 파일 구조는 `decode::mat_probe`에 있습니다. 분석기 `ssr-replay mat-analyze`는 `cfg(windows)` 밖에 두었습니다. 둘 다 합성 배치로 시험합니다.

## 2026-10-22 덧붙임: `plughost` 계열이 빠짐

ADR 0039로 플러그인 호스팅을 crates.io의 vst-audio에 맡깁니다. `plughost-sys`(181)와 `plughost-testplugin`(126)이 `unsafe-budget.toml`에서 빠지고, 격리 크레이트는 `ssr-sys`, `ssr-setup-sys`, `asio-sys` 셋이 됩니다. `upmix-sys`는 여전히 ADR 0020의 조건을 채울 때만 만듭니다.

- **Windows 의존성 게이트:** 허용 목록에서 `plughost-sys`를 뺍니다.
- **링크 규칙:** `plughost-sys`와 `plughost`의 규칙 대신, 플러그인을 적재하는 `vst-audio-worker-runtime`과 `vst-audio-worker-sys`는 `apps/ssr-plughost`만, `vst-audio`와 `vst-audio-transport-sys`는 앱만 링크합니다. 엔진은 unsafe가 없는 `vst-audio-ring`만 씁니다.
- **독립성:** `streamring`과 `ssr-plughost`는 `ssr-*`에 의존하지 않습니다.
