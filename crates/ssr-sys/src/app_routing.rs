#![allow(unsafe_code)]

//! Per-app render policy and session observations, never called on an RT thread.
//!
//! ABI facts (not a Microsoft compatibility guarantee), independently implemented
//! after inspecting these EarTrumpet declarations (no implementation copied):
//! <https://raw.githubusercontent.com/File-New-Project/EarTrumpet/master/EarTrumpet/Interop/MMDeviceAPI/IAudioPolicyConfigFactoryVariantFor21H2.cs>
//! <https://raw.githubusercontent.com/File-New-Project/EarTrumpet/master/EarTrumpet/Interop/MMDeviceAPI/IAudioPolicyConfigFactoryVariantForDownlevel.cs>
//! <https://raw.githubusercontent.com/File-New-Project/EarTrumpet/master/EarTrumpet/Interop/Helpers/AudioPolicyConfigFactoryImplFor21H2.cs>
//! <https://raw.githubusercontent.com/File-New-Project/EarTrumpet/master/EarTrumpet/Interop/Helpers/AudioPolicyConfigFactoryImplForDownlevel.cs>
//! Both interfaces have IInspectable (slots 0..5), 19 unused methods (6..24),
//! SetPersistedDefaultAudioEndpoint (25), GetPersistedDefaultAudioEndpoint (26),
//! then ClearAllPersistedApplicationDefaultEndpoints (27, deliberately absent).
//! Set takes an HSTRING handle by value, Get an owned HSTRING out pointer;
//! neither takes a UTF-16 character pointer. Both take (pid, flow, role) first.
//!
//! Microsoft says every WASAPI stream belongs to a session, including the
//! exclusive Initialize example. GetSessionEnumerator queries the audio engine
//! and explicitly warns it can miss newly notified sessions. No shared/exclusive
//! discriminator exists in the session interfaces; active is NOT shared. We do
//! not filter on sharing mode, but those documents do not guarantee enumeration
//! of every exclusive player. That behavior remains unverified on Windows.
//! This notification-free polling is best effort (ASIO/KS also bypass WASAPI).
//! <https://learn.microsoft.com/en-us/windows/win32/coreaudio/audio-sessions>
//! <https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-initialize>
//! <https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nf-audiopolicy-iaudiosessionmanager2-getsessionenumerator>
//! <https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nn-audiopolicy-iaudiosessioncontrol2>

use std::collections::BTreeMap;
use std::ffi::c_void;

use ssr_types::{AudioApp, DeviceId, HostError};
use windows::{
    Win32::{
        Foundation::{
            CLASS_E_CLASSNOTAVAILABLE, CloseHandle, E_NOINTERFACE, E_NOTIMPL, HANDLE,
            REGDB_E_CLASSNOTREG, RO_E_METADATA_NAME_NOT_FOUND, WAIT_TIMEOUT,
        },
        Media::Audio::{
            AudioSessionStateActive, AudioSessionStateExpired, DEVICE_STATE_ACTIVE,
            IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator,
            eConsole, eMultimedia, eRender,
        },
        System::{
            Com::{CLSCTX_ALL, CoCreateInstance},
            Threading::{
                OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW, WaitForSingleObject,
            },
            WinRT::{RO_INIT_MULTITHREADED, RoGetActivationFactory, RoInitialize, RoUninitialize},
        },
    },
    core::{GUID, HRESULT, HSTRING, IInspectable_Vtbl, IUnknown, Interface, PWSTR},
};

use crate::app_device_id::{consistent_roles, executable_basename, unwrap, wrap};
use crate::devices::ComGuard;

struct Process(HANDLE);

impl Process {
    fn open(pid: u32) -> Result<Self, HostError> {
        if pid == 0 {
            return Err(HostError::Failed("invalid app process id 0".into()));
        }
        // SAFETY: a nonzero pid, no inherited handle, query/synchronize rights only.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
        }
        .map_err(|error| failed("OpenProcess", error))?;
        let process = Self(handle);
        process.check_running()?;
        Ok(process)
    }

    fn check_running(&self) -> Result<(), HostError> {
        // SAFETY: owned process handle with SYNCHRONIZE; a zero-timeout query.
        if unsafe { WaitForSingleObject(self.0, 0) } != WAIT_TIMEOUT {
            return Err(HostError::Failed("app process is no longer running".into()));
        }
        Ok(())
    }

    fn exe(&self) -> Option<String> {
        let mut path = vec![0_u16; 32_768];
        let mut length = path.len() as u32;
        // SAFETY: owned query handle; writable buffer for the supplied UTF-16 count.
        unsafe {
            QueryFullProcessImageNameW(
                self.0,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut length,
            )
        }
        .ok()?;
        self.check_running().ok()?;
        executable_basename(&String::from_utf16(&path[..length as usize]).ok()?)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: OpenProcess transferred this handle; it is closed exactly once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

pub(crate) fn audio_apps() -> Vec<AudioApp> {
    let Ok(_com) = ComGuard::new() else {
        return Vec::new();
    };
    collect_apps().unwrap_or_default()
}

fn collect_apps() -> windows::core::Result<Vec<AudioApp>> {
    // SAFETY: this thread holds ComGuard; typed activation requests the enumerator IID.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }?;
    // SAFETY: live enumerator, render flow and active-state mask.
    let devices = unsafe { enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) }?;
    // SAFETY: live collection owned on this apartment.
    let count = unsafe { devices.GetCount() }?;
    let mut apps: BTreeMap<(u32, DeviceId), AudioApp> = BTreeMap::new();
    for index in 0..count {
        // SAFETY: index is below the collection's count.
        let Ok(device) = (unsafe { devices.Item(index) }) else {
            continue;
        };
        // SAFETY: live device; returned CoTaskMem string is freed by take_com_string.
        let Ok(id) = (unsafe { device.GetId() }) else {
            continue;
        };
        let id = DeviceId(crate::host::take_com_string(id));
        // SAFETY: live device, requested typed session manager, no activation parameters.
        let Ok(manager) = (unsafe { device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) })
        else {
            continue;
        };
        // SAFETY: live manager in the MTA; result has COM-owned lifetime.
        let Ok(sessions) = (unsafe { manager.GetSessionEnumerator() }) else {
            continue;
        };
        // SAFETY: live enumerator; GetCount also initializes its enumeration snapshot.
        let Ok(count) = (unsafe { sessions.GetCount() }) else {
            continue;
        };
        for index in 0..count {
            // SAFETY: index is in the session collection's range.
            let Ok(session) = (unsafe { sessions.GetSession(index) }) else {
                continue;
            };
            let Ok(session) = session.cast::<IAudioSessionControl2>() else {
                continue;
            };
            if let Some(app) = describe_session(&session, id.clone()) {
                apps.entry((app.pid, app.device.clone()))
                    .and_modify(|old| old.active |= app.active)
                    .or_insert(app);
            }
        }
    }
    Ok(apps.into_values().collect())
}

fn describe_session(session: &IAudioSessionControl2, device: DeviceId) -> Option<AudioApp> {
    // SAFETY: live typed session. windows 0.62.2 returns raw HRESULT here (not
    // Result<()>). Only S_FALSE (1) means non-system; S_OK (0) means system.
    // https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nf-audiopolicy-iaudiosessioncontrol2-issystemsoundssession
    if unsafe { session.IsSystemSoundsSession() }.0 != 1 {
        return None;
    }
    let mut pid = 0;
    // SAFETY: live typed session and writable DWORD. Preserve the raw status:
    // AUDCLNT_S_NO_SINGLE_PROCESS identifies only the original creator, not
    // every participant. Omit these ambiguous sessions rather than route one
    // participant as though it owned all audio in the session.
    // https://learn.microsoft.com/en-us/windows/win32/api/audiopolicy/nf-audiopolicy-iaudiosessioncontrol2-getprocessid
    if unsafe { (session.vtable().GetProcessId)(session.as_raw(), &mut pid) }.0 != 0 {
        return None;
    }
    let process = Process::open(pid).ok()?;
    let exe = process.exe()?;
    // SAFETY: live typed session; returned state is a value, not a pointer.
    let state = unsafe { session.GetState() }.ok()?;
    if state == AudioSessionStateExpired {
        return None;
    }
    // SAFETY: live session; the returned CoTaskMem string is consumed and freed.
    let display_name = unsafe { session.GetDisplayName() }
        .map(crate::host::take_com_string)
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| exe.clone());
    Some(AudioApp {
        pid,
        exe,
        display_name,
        device,
        active: state == AudioSessionStateActive,
    })
}

struct RoGuard;

impl RoGuard {
    fn new() -> Result<Self, HostError> {
        // SAFETY: non-UI control thread; S_OK and S_FALSE both require balancing.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
            .map_err(|error| failed("RoInitialize", error))?;
        Ok(Self)
    }
}

impl Drop for RoGuard {
    fn drop(&mut self) {
        // SAFETY: exactly one successful RoInitialize, same thread, interfaces gone.
        unsafe { RoUninitialize() };
    }
}

#[repr(C)]
struct PolicyVtbl {
    base: IInspectable_Vtbl,
    unused: [usize; 19],
    set: unsafe extern "system" fn(*mut c_void, u32, i32, i32, *mut c_void) -> HRESULT,
    get: unsafe extern "system" fn(*mut c_void, u32, i32, i32, *mut HSTRING) -> HRESULT,
}

#[repr(transparent)]
#[derive(Clone)]
struct Policy(IUnknown);
#[repr(transparent)]
#[derive(Clone)]
struct Downlevel(IUnknown);

// SAFETY: transparent owning COM pointer, verified 21H2 IID and IInspectable vtable.
unsafe impl Interface for Policy {
    type Vtable = PolicyVtbl;
    const IID: GUID = GUID::from_u128(0xab3d4648_e242_459f_b02f_541c70306324);
}
// SAFETY: downlevel has the same verified ABI through slot 26, but its own IID.
unsafe impl Interface for Downlevel {
    type Vtable = PolicyVtbl;
    const IID: GUID = GUID::from_u128(0x2a59116d_6c4f_45e0_a74f_707e3fef9258);
}

fn unavailable(code: HRESULT) -> bool {
    matches!(
        code,
        E_NOINTERFACE
            | E_NOTIMPL
            | REGDB_E_CLASSNOTREG
            | CLASS_E_CLASSNOTAVAILABLE
            | RO_E_METADATA_NAME_NOT_FOUND
    )
}

fn failed(operation: &str, error: windows::core::Error) -> HostError {
    HostError::Failed(format!("{operation}: {error}"))
}

fn policy_result(operation: &str, status: HRESULT) -> Result<(), HostError> {
    if unavailable(status) {
        Err(HostError::Unsupported)
    } else {
        status.ok().map_err(|error| failed(operation, error))
    }
}

impl Policy {
    fn activate() -> Result<Self, HostError> {
        let name = HSTRING::from("Windows.Media.Internal.AudioPolicyConfig");
        // SAFETY: RoGuard is live, the runtime name and verified IID select this vtable.
        let recent = unsafe { RoGetActivationFactory::<Self>(&name) };
        if let Ok(policy) = recent {
            return Ok(policy);
        }
        // SAFETY: same runtime class, requesting the distinct downlevel IID directly.
        let older = unsafe { RoGetActivationFactory::<Downlevel>(&name) };
        match older {
            // Both verified layouts are identical through the last slot we call.
            // No QueryInterface using Policy::IID is performed on this wrapper.
            Ok(policy) => Ok(Self(policy.0)),
            // Neither known ABI can be activated on this host. The supervisor
            // falls back to guidance instead of retrying a private interface.
            Err(_) => Err(HostError::Unsupported),
        }
    }

    fn get(&self, pid: u32, role: i32) -> Result<HSTRING, HostError> {
        let mut id = HSTRING::new();
        // SAFETY: verified slot 26; HSTRING is repr(transparent) over its handle,
        // initialized null and receives ownership. Drop frees it even on failure.
        let status = unsafe { (self.vtable().get)(self.as_raw(), pid, eRender.0, role, &mut id) };
        policy_result("GetPersistedDefaultAudioEndpoint", status)?;
        Ok(id)
    }

    fn set(&self, pid: u32, role: i32, id: &HSTRING) -> Result<(), HostError> {
        // SAFETY: verified slot 25; HSTRING's ABI is a handle by value, obtained
        // exactly as windows' generated WinRT bindings do, not id.as_ptr().
        // The string stays owned here throughout the call. Empty = null = clear
        // this app/flow/role only. Communications and clear-all are never called.
        let handle = unsafe { std::mem::transmute_copy::<HSTRING, *mut c_void>(id) };
        // SAFETY: self owns the verified interface and id keeps handle alive.
        let status = unsafe { (self.vtable().set)(self.as_raw(), pid, eRender.0, role, handle) };
        policy_result("SetPersistedDefaultAudioEndpoint", status)
    }
}

pub(crate) fn app_entrance(pid: u32) -> Result<Option<DeviceId>, HostError> {
    let _ro = RoGuard::new()?;
    let policy = Policy::activate()?;
    let _process = Process::open(pid)?;
    let console = policy.get(pid, eConsole.0)?;
    let multimedia = policy.get(pid, eMultimedia.0)?;
    Ok(consistent_roles(
        decode_id(&console)?,
        decode_id(&multimedia)?,
    ))
}

fn decode_id(id: &HSTRING) -> Result<Option<DeviceId>, HostError> {
    let text = String::from_utf16(id).map_err(|error| HostError::Failed(error.to_string()))?;
    unwrap(&text)
}

pub(crate) fn set_app_entrance(pid: u32, device: Option<&DeviceId>) -> Result<(), HostError> {
    let target = device
        .map(wrap)
        .transpose()?
        .map_or_else(HSTRING::new, HSTRING::from);
    let _ro = RoGuard::new()?;
    let policy = Policy::activate()?;
    // Holding the process handle prevents a reused PID from being targeted on rollback.
    let process = Process::open(pid)?;
    if let Some(device) = device {
        require_active_render(device)?;
    }
    let roles = [eConsole.0, eMultimedia.0];
    let previous = [policy.get(pid, roles[0])?, policy.get(pid, roles[1])?];
    let mut written: Vec<usize> = Vec::new();
    let result = (|| {
        for (index, role) in roles.into_iter().enumerate() {
            process.check_running()?;
            if policy.get(pid, role)? != previous[index] {
                return Err(HostError::Conflict);
            }
            if previous[index] != target {
                policy.set(pid, role, &target)?;
                written.push(index);
            }
        }
        // Check each role, including clear: app_entrance(None) alone cannot
        // distinguish both cleared from disagreement between the two roles.
        for role in roles {
            if decode_id(&policy.get(pid, role)?)?.as_ref() != device {
                return Err(HostError::Conflict);
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        // No compare-and-swap exists. Restore only our successful writes
        // whose current value is still ours; preserve observed concurrent
        // user edits and all other roles/apps. A narrow read/write race remains.
        let mut rollback = Vec::new();
        for index in written.into_iter().rev() {
            if let Err(error) = process.check_running().and_then(|()| {
                let current = policy.get(pid, roles[index])?;
                if current == target {
                    policy.set(pid, roles[index], &previous[index])?;
                }
                Ok(())
            }) {
                rollback.push(error.to_string());
            }
        }
        return if matches!(error, HostError::Conflict) {
            // A conflict is a user's concurrent preference change. Even when a
            // rollback call fails, preserve that verdict so policy does not
            // reassert on the next poll.
            Err(HostError::Conflict)
        } else if rollback.is_empty() {
            Err(error)
        } else {
            Err(HostError::Failed(format!(
                "{error}; rollback: {}",
                rollback.join("; ")
            )))
        };
    }
    Ok(())
}

fn require_active_render(id: &DeviceId) -> Result<(), HostError> {
    // SAFETY: RoGuard initialized COM too; typed activation, no aggregation.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .map_err(|error| failed("MMDeviceEnumerator", error))?;
    // Enumerating render endpoints rather than GetDevice also rejects capture ids.
    // SAFETY: live enumerator, render flow, active-state mask.
    let devices = unsafe { enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) }
        .map_err(|error| failed("EnumAudioEndpoints", error))?;
    // SAFETY: live collection on this apartment.
    let count = unsafe { devices.GetCount() }.map_err(|error| failed("GetCount", error))?;
    for index in 0..count {
        // SAFETY: index is below the count of this live collection.
        let device = unsafe { devices.Item(index) }.map_err(|error| failed("Item", error))?;
        // SAFETY: live device; take_com_string frees the returned allocation.
        let value = unsafe { device.GetId() }.map_err(|error| failed("GetId", error))?;
        if crate::host::take_com_string(value) == id.0 {
            return Ok(());
        }
    }
    Err(HostError::Failed(format!(
        "render endpoint {:?} is not active",
        id.0
    )))
}
