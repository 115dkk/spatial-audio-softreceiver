//! Safe, thread-bound ISpatialAudioClient observation port. No MAT interpretation.
#![allow(unsafe_code)]
use crate::devices::ComGuard;
use std::mem::ManuallyDrop;
use windows::Win32::{
    Foundation::{CloseHandle, E_INVALIDARG, E_UNEXPECTED, HANDLE, WAIT_OBJECT_0},
    Media::Audio::*,
    System::{
        Com::{
            BLOB, CLSCTX_ALL,
            StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0},
        },
        Threading::{CreateEventW, WaitForSingleObject},
        Variant::VT_BLOB,
    },
};
use windows::core::{Interface, Result};

#[derive(Clone, Debug)]
pub struct StaticPosition {
    pub kind: &'static str,
    pub object_type: u32,
    pub position: Option<[f32; 3]>,
    pub hresult: i32,
}
#[derive(Clone, Debug)]
pub struct Info {
    pub endpoint_id: String,
    pub max_dynamic: u32,
    pub static_mask: u32,
    pub format_hresult: i32,
    pub available_hresult: i32,
    pub positions: Vec<StaticPosition>,
}
#[derive(Debug)]
struct Event(HANDLE);
impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: owned CreateEventW handle, closed exactly once after the stream.
        let _ = unsafe { CloseHandle(self.0) };
    }
}
#[derive(Debug)]
pub struct Client {
    client: ISpatialAudioClient,
    pub info: Info,
    _com: ComGuard,
}
#[derive(Debug)]
pub enum OpenError {
    Endpoint(String),
    SpatialUnavailable(String),
}
impl Client {
    pub fn open(endpoint: u16) -> std::result::Result<Self, OpenError> {
        let com = ComGuard::new().map_err(|e| OpenError::Endpoint(e.to_string()))?;
        let device = crate::host::probe_endpoint(endpoint)
            .map_err(|e| OpenError::Endpoint(e.to_string()))?;
        // SAFETY: device is live on this apartment, no activation parameters.
        let client: ISpatialAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }
            .map_err(|e| OpenError::SpatialUnavailable(e.to_string()))?;
        // SAFETY: live interface, no pointer inputs.
        let max_dynamic = unsafe { client.GetMaxDynamicObjectCount() }
            .map_err(|e| OpenError::SpatialUnavailable(e.to_string()))?;
        // SAFETY: live interface, no pointer inputs.
        let mask = unsafe { client.GetNativeStaticObjectTypeMask() }
            .map_err(|e| OpenError::SpatialUnavailable(e.to_string()))?;
        let format = wave();
        // SAFETY: format remains live for the synchronous call.
        let format_hresult = status(unsafe { client.IsAudioObjectFormatSupported(&format) });
        // SAFETY: IID is valid and no auxiliary parameters are supplied.
        let available_hresult = status(unsafe {
            client.IsSpatialAudioStreamAvailable(&ISpatialAudioObjectRenderStream::IID, None)
        });
        let names = [
            "FrontLeft",
            "FrontRight",
            "FrontCenter",
            "LowFrequency",
            "SideLeft",
            "SideRight",
            "BackLeft",
            "BackRight",
            "TopFrontLeft",
            "TopFrontRight",
            "TopBackLeft",
            "TopBackRight",
            "BottomFrontLeft",
            "BottomFrontRight",
            "BottomBackLeft",
            "BottomBackRight",
            "BackCenter",
        ];
        let mut positions = Vec::new();
        for (index, kind) in names.into_iter().enumerate() {
            let object_type = 1u32 << (index + 1);
            let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
            // SAFETY: three distinct live f32 outputs; documented static type bit.
            let result = unsafe {
                client.GetStaticObjectPosition(
                    AudioObjectType(object_type as i32),
                    &mut x,
                    &mut y,
                    &mut z,
                )
            };
            let hresult = status(result);
            positions.push(StaticPosition {
                kind,
                object_type,
                position: (hresult == 0).then_some([x, y, z]),
                hresult,
            });
        }
        let endpoint_id = wasapi::Device::from_immdevice(device)
            .and_then(|d| d.get_id())
            .map_err(|e| OpenError::Endpoint(e.to_string()))?;
        Ok(Self {
            client,
            info: Info {
                endpoint_id,
                max_dynamic,
                static_mask: mask.0 as u32,
                format_hresult,
                available_hresult,
                positions,
            },
            _com: com,
        })
    }
    pub fn activate(self) -> Result<Stream> {
        let format = wave();
        // SAFETY: unnamed auto-reset event, no borrowed security attributes.
        let event = Event(unsafe { CreateEventW(None, false, false, None) }?);
        let mut params = SpatialAudioObjectRenderStreamActivationParams {
            ObjectFormat: &format,
            StaticObjectTypeMask: AudioObjectType(self.info.static_mask as i32),
            MinDynamicObjectCount: self.info.max_dynamic,
            MaxDynamicObjectCount: self.info.max_dynamic,
            Category: AudioCategory_GameEffects,
            EventHandle: event.0,
            NotifyObject: ManuallyDrop::new(None),
        };
        // The blob borrows params for this synchronous call. Do not PropVariantClear:
        // Windows does not own this stack storage (Microsoft rendering example).
        let variant = PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_BLOB,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: PROPVARIANT_0_0_0 {
                        blob: BLOB {
                            cbSize: std::mem::size_of_val(&params) as u32,
                            pBlobData: std::ptr::from_mut(&mut params).cast(),
                        },
                    },
                }),
            },
        };
        // SAFETY: typed VT_BLOB describes live params/format; event outlives stream.
        let stream = unsafe { self.client.ActivateSpatialAudioStream(&variant) }?;
        Ok(Stream {
            stream,
            objects: Vec::with_capacity(
                self.info.static_mask.count_ones() as usize + self.info.max_dynamic as usize,
            ),
            event,
            client: self,
            updating: false,
            frames: 0,
        })
    }
}
fn wave() -> WAVEFORMATEX {
    WAVEFORMATEX {
        wFormatTag: 3,
        nChannels: 1,
        nSamplesPerSec: 48000,
        nAvgBytesPerSec: 192000,
        nBlockAlign: 4,
        wBitsPerSample: 32,
        cbSize: 0,
    }
}
fn status(result: Result<()>) -> i32 {
    result.err().map_or(0, |e| e.code().0)
}

#[derive(Debug)]
pub struct Stream {
    stream: ISpatialAudioObjectRenderStream,
    objects: Vec<ISpatialAudioObject>,
    event: Event,
    client: Client,
    updating: bool,
    frames: u32,
}
impl Stream {
    pub fn start(&self) -> Result<()> {
        // SAFETY: live activated stream and event.
        unsafe { self.stream.Start() }
    }
    pub fn begin(&mut self) -> Result<(u32, u32)> {
        if self.updating {
            return Err(E_UNEXPECTED.into());
        }
        // SAFETY: event is owned and remains valid for the wait.
        if unsafe { WaitForSingleObject(self.event.0, 2000) } != WAIT_OBJECT_0 {
            return Err(E_UNEXPECTED.into());
        }
        let (mut available, mut frames) = (0, 0);
        // SAFETY: distinct live outputs and no outstanding update.
        unsafe {
            self.stream
                .BeginUpdatingAudioObjects(&mut available, &mut frames)
        }?;
        self.updating = true;
        self.frames = frames;
        Ok((available, frames))
    }
    pub fn activate_object(&mut self, object_type: u32) -> Result<()> {
        if !self.updating {
            return Err(E_UNEXPECTED.into());
        }
        let limit = self.client.info.static_mask.count_ones() as usize
            + self.client.info.max_dynamic as usize;
        if self.objects.len() >= limit {
            return Err(E_INVALIDARG.into());
        }
        // SAFETY: within Begin/End, valid static mask bit or dynamic type.
        let object = unsafe {
            self.stream
                .ActivateSpatialAudioObject(AudioObjectType(object_type as i32))
        }?;
        self.objects.push(object);
        Ok(())
    }
    /// The callback borrows the buffer only during this update; no allocation.
    pub fn fill(
        &mut self,
        index: usize,
        position: Option<[f32; 3]>,
        volume: f32,
        fill: impl FnOnce(&mut [f32]),
    ) -> Result<()> {
        if !self.updating {
            return Err(E_UNEXPECTED.into());
        }
        let object = self
            .objects
            .get(index)
            .ok_or_else(|| windows::core::Error::from_hresult(E_INVALIDARG))?;
        if let Some([x, y, z]) = position {
            // SAFETY: live dynamic object, finite caller-supplied coordinates.
            unsafe { object.SetPosition(x, y, z) }?;
        }
        // SAFETY: live object and caller-supplied volume.
        unsafe { object.SetVolume(volume) }?;
        let (mut pointer, mut length) = (std::ptr::null_mut(), 0);
        // SAFETY: outputs live; only called between Begin and End.
        unsafe { object.GetBuffer(&mut pointer, &mut length) }?;
        if pointer.is_null()
            || length as usize != self.frames as usize * 4
            || !(pointer as usize).is_multiple_of(std::mem::align_of::<f32>())
        {
            return Err(E_UNEXPECTED.into());
        }
        // SAFETY: Windows supplies a uniquely writable float32 buffer of checked size;
        // the borrow cannot escape the callback or survive EndUpdatingAudioObjects.
        let samples =
            unsafe { std::slice::from_raw_parts_mut(pointer.cast::<f32>(), self.frames as usize) };
        fill(samples);
        Ok(())
    }
    pub fn end(&mut self) -> Result<()> {
        if !self.updating {
            return Err(E_UNEXPECTED.into());
        }
        self.updating = false;
        // SAFETY: balances the successful Begin exactly once, including error paths.
        unsafe { self.stream.EndUpdatingAudioObjects() }
    }
    pub fn stop(&mut self) -> Result<()> {
        if self.updating {
            self.end()?;
        }
        // SAFETY: live stream, may be stopped on cleanup as well.
        unsafe { self.stream.Stop() }
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
