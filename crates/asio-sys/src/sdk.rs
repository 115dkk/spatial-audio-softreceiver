// SPDX-License-Identifier: GPL-3.0-only
// Hand-transcribed from Steinberg ASIO SDK 2.3.4 common/asio.h and common/iasiodrv.h.
// SHA-256 d5ebf0c20dd2c5f43771fd0c1418f4b361bf52434ee670097cfa6b3a335e2eca.
// Windows: NATIVE_INT64=0, IEEE754_64FLOAT=1, #pragma pack(push,4).
#![allow(dead_code, non_upper_case_globals, non_snake_case, unsafe_code)]
use std::ffi::c_void;

pub const ASIOFalse: i32 = 0;
pub const ASIOTrue: i32 = 1;
pub const ASIOSTInt16MSB: i32 = 0;
pub const ASIOSTInt24MSB: i32 = 1;
pub const ASIOSTInt32MSB: i32 = 2;
pub const ASIOSTFloat32MSB: i32 = 3;
pub const ASIOSTFloat64MSB: i32 = 4;
pub const ASIOSTInt32MSB16: i32 = 8;
pub const ASIOSTInt32MSB18: i32 = 9;
pub const ASIOSTInt32MSB20: i32 = 10;
pub const ASIOSTInt32MSB24: i32 = 11;
pub const ASIOSTInt16LSB: i32 = 16;
pub const ASIOSTInt24LSB: i32 = 17;
pub const ASIOSTInt32LSB: i32 = 18;
pub const ASIOSTFloat32LSB: i32 = 19;
pub const ASIOSTFloat64LSB: i32 = 20;
pub const ASIOSTInt32LSB16: i32 = 24;
pub const ASIOSTInt32LSB18: i32 = 25;
pub const ASIOSTInt32LSB20: i32 = 26;
pub const ASIOSTInt32LSB24: i32 = 27;
pub const ASIOSTDSDInt8LSB1: i32 = 32;
pub const ASIOSTDSDInt8MSB1: i32 = 33;
pub const ASIOSTDSDInt8NER8: i32 = 40;
pub const ASIOSTLastEntry: i32 = 41;
pub const ASE_OK: i32 = 0;
pub const ASE_SUCCESS: i32 = 0x3f4847a0;
pub const ASE_NotPresent: i32 = -1000;
pub const ASE_HWMalfunction: i32 = -999;
pub const ASE_InvalidParameter: i32 = -998;
pub const ASE_InvalidMode: i32 = -997;
pub const ASE_SPNotAdvancing: i32 = -996;
pub const ASE_NoClock: i32 = -995;
pub const ASE_NoMemory: i32 = -994;
pub const kTcValid: i32 = 1;
pub const kTcRunning: i32 = 2;
pub const kTcReverse: i32 = 4;
pub const kTcOnspeed: i32 = 8;
pub const kTcStill: i32 = 16;
pub const kTcSpeedValid: i32 = 256;
pub const kSystemTimeValid: i32 = 1;
pub const kSamplePositionValid: i32 = 2;
pub const kSampleRateValid: i32 = 4;
pub const kSpeedValid: i32 = 8;
pub const kSampleRateChanged: i32 = 16;
pub const kClockSourceChanged: i32 = 32;
pub const kAsioSelectorSupported: i32 = 1;
pub const kAsioEngineVersion: i32 = 2;
pub const kAsioResetRequest: i32 = 3;
pub const kAsioBufferSizeChange: i32 = 4;
pub const kAsioResyncRequest: i32 = 5;
pub const kAsioLatenciesChanged: i32 = 6;
pub const kAsioSupportsTimeInfo: i32 = 7;
pub const kAsioSupportsTimeCode: i32 = 8;
pub const kAsioMMCCommand: i32 = 9;
pub const kAsioSupportsInputMonitor: i32 = 10;
pub const kAsioSupportsInputGain: i32 = 11;
pub const kAsioSupportsInputMeter: i32 = 12;
pub const kAsioSupportsOutputGain: i32 = 13;
pub const kAsioSupportsOutputMeter: i32 = 14;
pub const kAsioOverload: i32 = 15;
pub const kAsioNumMessageSelectors: i32 = 16;
pub const kAsioEnableTimeCodeRead: i32 = 1;
pub const kAsioDisableTimeCodeRead: i32 = 2;
pub const kAsioSetInputMonitor: i32 = 3;
pub const kAsioTransport: i32 = 4;
pub const kAsioSetInputGain: i32 = 5;
pub const kAsioGetInputMeter: i32 = 6;
pub const kAsioSetOutputGain: i32 = 7;
pub const kAsioGetOutputMeter: i32 = 8;
pub const kAsioCanInputMonitor: i32 = 9;
pub const kAsioCanTimeInfo: i32 = 10;
pub const kAsioCanTimeCode: i32 = 11;
pub const kAsioCanTransport: i32 = 12;
pub const kAsioCanInputGain: i32 = 13;
pub const kAsioCanInputMeter: i32 = 14;
pub const kAsioCanOutputGain: i32 = 15;
pub const kAsioCanOutputMeter: i32 = 16;
pub const kAsioOptionalOne: i32 = 17;
pub const kAsioSetIoFormat: i32 = 0x23111961;
pub const kAsioGetIoFormat: i32 = 0x23111983;
pub const kAsioCanDoIoFormat: i32 = 0x23112004;
pub const kAsioCanReportOverload: i32 = 0x24042012;
pub const kAsioGetInternalBufferSamples: i32 = 0x25042012;
pub const kTransStart: i32 = 1;
pub const kTransStop: i32 = 2;
pub const kTransLocate: i32 = 3;
pub const kTransPunchIn: i32 = 4;
pub const kTransPunchOut: i32 = 5;
pub const kTransArmOn: i32 = 6;
pub const kTransArmOff: i32 = 7;
pub const kTransMonitorOn: i32 = 8;
pub const kTransMonitorOff: i32 = 9;
pub const kTransArm: i32 = 10;
pub const kTransMonitor: i32 = 11;
pub const kASIOFormatInvalid: i32 = -1;
pub const kASIOPCMFormat: i32 = 0;
pub const kASIODSDFormat: i32 = 1;

#[repr(C, packed(4))]
#[derive(Clone, Copy, Debug, Default)]
pub struct Samples {
    pub hi: u32,
    pub lo: u32,
}
impl From<u64> for Samples {
    fn from(v: u64) -> Self {
        Self {
            hi: (v >> 32) as u32,
            lo: v as u32,
        }
    }
}
#[repr(C, packed(4))]
#[derive(Clone, Copy, Debug, Default)]
pub struct TimeInfo {
    pub speed: f64,
    pub systemTime: Samples,
    pub samplePosition: Samples,
    pub sampleRate: f64,
    pub flags: u32,
    pub reserved: [u8; 12],
}
#[repr(C, packed(4))]
#[derive(Clone, Copy, Debug)]
pub struct TimeCode {
    pub speed: f64,
    pub timeCodeSamples: Samples,
    pub flags: u32,
    pub future: [u8; 64],
}
#[repr(C, packed(4))]
#[derive(Clone, Copy, Debug)]
pub struct Time {
    pub reserved: [i32; 4],
    pub timeInfo: TimeInfo,
    pub timeCode: TimeCode,
}
impl Time {
    pub fn new(position: u64, nanos: u64, rate: f64) -> Self {
        Self {
            reserved: [0; 4],
            timeInfo: TimeInfo {
                speed: 1.0,
                systemTime: nanos.into(),
                samplePosition: position.into(),
                sampleRate: rate,
                flags: 7,
                reserved: [0; 12],
            },
            timeCode: TimeCode {
                speed: 0.0,
                timeCodeSamples: 0.into(),
                flags: 0,
                future: [0; 64],
            },
        }
    }
}
#[repr(C, packed(4))]
#[derive(Clone, Copy, Debug)]
pub struct Callbacks {
    pub bufferSwitch: Option<unsafe extern "C" fn(i32, i32)>,
    pub sampleRateDidChange: Option<unsafe extern "C" fn(f64)>,
    pub asioMessage: Option<unsafe extern "C" fn(i32, i32, *mut c_void, *mut f64) -> i32>,
    pub bufferSwitchTimeInfo: Option<unsafe extern "C" fn(*mut Time, i32, i32) -> *mut Time>,
}
#[repr(C, packed(4))]
#[derive(Clone, Copy, Debug, Default)]
pub struct ClockSource {
    pub index: i32,
    pub associatedChannel: i32,
    pub associatedGroup: i32,
    pub isCurrentSource: i32,
    pub name: [u8; 32],
}
#[repr(C, packed(4))]
#[derive(Clone, Copy, Debug, Default)]
pub struct ChannelInfo {
    pub channel: i32,
    pub isInput: i32,
    pub isActive: i32,
    pub channelGroup: i32,
    pub sample_type: i32,
    pub name: [u8; 32],
}
#[repr(C, packed(4))]
#[derive(Clone, Copy, Debug)]
pub struct BufferInfo {
    pub isInput: i32,
    pub channelNum: i32,
    pub buffers: [*mut c_void; 2],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}
#[repr(C)]
#[derive(Debug)]
pub struct UnknownVtable {
    pub query: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
}
// x64 Windows has a single calling convention. x86 is deliberately unsupported.
#[repr(C)]
#[derive(Debug)]
pub struct AsioVtable {
    pub unknown: UnknownVtable,
    pub init: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
    pub getDriverName: unsafe extern "system" fn(*mut c_void, *mut u8),
    pub getDriverVersion: unsafe extern "system" fn(*mut c_void) -> i32,
    pub getErrorMessage: unsafe extern "system" fn(*mut c_void, *mut u8),
    pub start: unsafe extern "system" fn(*mut c_void) -> i32,
    pub stop: unsafe extern "system" fn(*mut c_void) -> i32,
    pub getChannels: unsafe extern "system" fn(*mut c_void, *mut i32, *mut i32) -> i32,
    pub getLatencies: unsafe extern "system" fn(*mut c_void, *mut i32, *mut i32) -> i32,
    pub getBufferSize:
        unsafe extern "system" fn(*mut c_void, *mut i32, *mut i32, *mut i32, *mut i32) -> i32,
    pub canSampleRate: unsafe extern "system" fn(*mut c_void, f64) -> i32,
    pub getSampleRate: unsafe extern "system" fn(*mut c_void, *mut f64) -> i32,
    pub setSampleRate: unsafe extern "system" fn(*mut c_void, f64) -> i32,
    pub getClockSources: unsafe extern "system" fn(*mut c_void, *mut ClockSource, *mut i32) -> i32,
    pub setClockSource: unsafe extern "system" fn(*mut c_void, i32) -> i32,
    pub getSamplePosition:
        unsafe extern "system" fn(*mut c_void, *mut Samples, *mut Samples) -> i32,
    pub getChannelInfo: unsafe extern "system" fn(*mut c_void, *mut ChannelInfo) -> i32,
    pub createBuffers:
        unsafe extern "system" fn(*mut c_void, *mut BufferInfo, i32, i32, *const Callbacks) -> i32,
    pub disposeBuffers: unsafe extern "system" fn(*mut c_void) -> i32,
    pub controlPanel: unsafe extern "system" fn(*mut c_void) -> i32,
    pub future: unsafe extern "system" fn(*mut c_void, i32, *mut c_void) -> i32,
    pub outputReady: unsafe extern "system" fn(*mut c_void) -> i32,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_sdk_struct_layouts() {
        assert_eq!(size_of::<Samples>(), 8);
        assert_eq!(align_of::<TimeInfo>(), 4);
        assert_eq!(size_of::<TimeInfo>(), 48);
        assert_eq!(size_of::<TimeCode>(), 84);
        assert_eq!(size_of::<Time>(), 148);
        assert_eq!(std::mem::offset_of!(Time, timeInfo), 16);
        assert_eq!(std::mem::offset_of!(Time, timeCode), 64);
        assert_eq!(size_of::<ChannelInfo>(), 52);
        assert_eq!(size_of::<ClockSource>(), 48);
        if cfg!(target_pointer_width = "64") {
            assert_eq!(size_of::<BufferInfo>(), 24);
            assert_eq!(size_of::<Callbacks>(), 32);
            assert_eq!(size_of::<AsioVtable>(), 24 * 8);
        }
        assert_eq!(std::mem::offset_of!(BufferInfo, buffers), 8);
    }
}
