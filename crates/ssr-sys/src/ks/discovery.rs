#![allow(unsafe_code)]

//! Resolve only the endpoint's connected KS graph. No friendly-name or device
//! substring matching and no global 'first render pin' fallback.
use super::io::{self, Control, Handle};
use ssr_types::{HostError, ports::KsTarget};
use std::{collections::HashSet, mem::size_of};
use windows::{
    Win32::{
        Media::{
            Audio::{IDeviceTopology, IMMDeviceEnumerator, IPart, MMDeviceEnumerator},
            KernelStreaming::*,
        },
        System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree},
    },
    core::{Interface, PCWSTR, PWSTR},
};

fn failure(e: impl std::fmt::Debug) -> HostError {
    HostError::Failed(format!("KS discovery: {e:?}"))
}
fn string(value: PWSTR) -> Result<String, HostError> {
    // SAFETY: COM returns a terminated CoTaskMem string owned by this caller.
    let result = unsafe { value.to_string() }.map_err(failure);
    // SAFETY: the COM allocation is released exactly once after copying.
    unsafe { CoTaskMemFree(Some(value.0.cast())) };
    result
}
fn path(value: String) -> String {
    // KSPIN_PHYSICALCONNECTION uses an NT symbolic link; CreateFile uses DOS.
    if let Some(rest) = value.strip_prefix("\\??\\") {
        format!("\\\\?\\{rest}")
    } else {
        value
    }
}

pub fn kernel_target(endpoint: &str) -> Result<KsTarget, HostError> {
    if endpoint.is_empty() || endpoint.contains('\0') {
        return Err(failure("invalid endpoint id"));
    }
    let _com = crate::devices::ComGuard::new().map_err(failure)?;
    // SAFETY: initialized COM apartment, known enumerator class/interface.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(failure)?;
    let endpoint: Vec<u16> = endpoint.encode_utf16().chain([0]).collect();
    // SAFETY: terminated endpoint id and live enumerator.
    let device = unsafe { enumerator.GetDevice(PCWSTR(endpoint.as_ptr())) }.map_err(failure)?;
    // SAFETY: live endpoint, documented topology activation.
    let topology: IDeviceTopology =
        unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(failure)?;
    // SAFETY: live topology.
    let count = unsafe { topology.GetConnectorCount() }.map_err(failure)?;
    let mut seeds = Vec::new();
    for index in 0..count {
        // SAFETY: index below the returned connector count.
        let connector = unsafe { topology.GetConnector(index) }.map_err(failure)?;
        // SAFETY: live connector; returns an owned COM interface.
        let Ok(connected) = (unsafe { connector.GetConnectedTo() }) else {
            continue;
        };
        let part: IPart = connected.cast().map_err(failure)?;
        // SAFETY: live part; low 16 bits identify the KS part (Microsoft IKsControl guide).
        let pin = unsafe { part.GetLocalId() }.map_err(failure)? & 0xffff;
        // SAFETY: live connector; allocated device path is copied/freed below.
        let filter = string(unsafe { connector.GetDeviceIdConnectedTo() }.map_err(failure)?)?;
        seeds.push((path(filter), pin));
    }
    let mut control = Control::new().map_err(failure)?;
    let mut visited = HashSet::new();
    let mut candidates = Vec::new();
    while let Some((filter, bridge)) = seeds.pop() {
        if !visited.insert((filter.to_lowercase(), bridge)) {
            continue;
        }
        if visited.len() > 32 {
            return Err(failure("physical graph exceeds 32 connections"));
        }
        let handle = Handle::filter(&filter).map_err(failure)?;
        let count: u32 =
            pin_value(&mut control, &handle, 0, KSPROPERTY_PIN_CTYPES).map_err(failure)?;
        if count > 1024 || bridge >= count {
            return Err(failure("invalid KS pin count/id"));
        }
        let connections = io::variable(
            &mut control,
            &handle,
            &io::id(
                KSPROPSETID_Topology,
                KSPROPERTY_TOPOLOGY_CONNECTIONS.0 as u32,
                KSPROPERTY_TYPE_GET,
            ),
        )
        .ok();
        // Stay in the connected component of the endpoint's bridge pin. Edges
        // inside a filter are traversed undirected, physical edges cross filters.
        let reachable = connected_pins(connections.as_deref(), bridge, count);
        for pin in reachable {
            if render_pin(&mut control, &handle, pin) {
                let target = KsTarget {
                    filter: filter.clone(),
                    pin,
                };
                if !candidates.contains(&target) {
                    candidates.push(target);
                }
            }
            if let Ok(bytes) = io::variable(
                &mut control,
                &handle,
                &io::pin_property(pin, KSPROPERTY_PIN_PHYSICALCONNECTION),
            ) && let Some((filter, next)) = super::selection::physical(&bytes)
            {
                seeds.push((path(filter), next));
            }
        }
    }
    match candidates.as_slice() {
        [target] => Ok(target.clone()),
        [] => Err(failure("no connected PCM render pin")),
        _ => Err(failure(
            "ambiguous connected render pins; refusing to choose an unrelated output",
        )),
    }
}

use super::selection::connected_pins;

pub(super) fn pin_value<O: Default + io::Pod>(
    control: &mut Control,
    filter: &Handle,
    pin: u32,
    property: KSPROPERTY_PIN,
) -> Result<O, windows::Win32::Foundation::WIN32_ERROR> {
    let mut value = O::default();
    let size = control.query(filter, &io::pin_property(pin, property), &mut value)?;
    if size as usize != size_of::<O>() {
        return Err(windows::Win32::Foundation::WIN32_ERROR(13));
    }
    Ok(value)
}
pub(super) fn render_pin(control: &mut Control, filter: &Handle, pin: u32) -> bool {
    let flow = pin_value::<KSPIN_DATAFLOW>(control, filter, pin, KSPROPERTY_PIN_DATAFLOW);
    let communication =
        pin_value::<KSPIN_COMMUNICATION>(control, filter, pin, KSPROPERTY_PIN_COMMUNICATION);
    match (flow, communication) {
        (Ok(flow), Ok(communication)) => super::selection::render_pin(flow.0, communication.0),
        _ => false,
    }
}
