//! The supervisor's [`PluginHost`] port (ADR 0035): on Windows,
//! `ssr-plughost.exe` beside the app through `vst_audio::Host`; on other
//! systems, a port that hosts nothing.
//!
//! Starts are [`vst_audio::LoadTask`]s polled by the control thread. The
//! high-level instance owns worker supervision, the 0.5/2/8-second watch
//! policy, state selection and recovery hand-over; the adapter maps its events
//! to the supervisor port. The port's `NeedsRespawn` variant remains for other
//! adapters and tests, but the Windows adapter never emits it.
//!
//! Plugin states (ADR 0021) are saved as the instance returns them. Each
//! endpoint has two files in the state directory,
//! `upmix-<endpoint>.state` and `upmix-<endpoint>.previous.state`: a new state
//! is written beside them and renamed over the newest, which becomes the
//! previous one. A rate or latency handoff asks the old instance to save and
//! stop, then restores the newest state in the new instance.

use std::fs;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use ssr_types::Speaker;
use ssr_types::app::PluginFormat;
use supervisor::upmix::{PluginClass, PluginHost, PluginHostError, PluginInstance, PluginSetting};
use vst_audio_protocol::ProcessingSetup;

use crate::log;
use crate::paths;

/// The port where plugins cannot be hosted: every scan and start fails with
/// [`PluginHostError::Unavailable`].
#[derive(Clone, Copy, Debug, Default)]
pub struct UnavailablePluginHost;

impl PluginHost for UnavailablePluginHost {
    fn scan(
        &mut self,
        _path: &Path,
        _format: PluginFormat,
    ) -> Result<Vec<PluginClass>, PluginHostError> {
        Err(PluginHostError::Unavailable)
    }

    fn start(
        &mut self,
        _endpoint: u16,
        _plugin: &PluginSetting,
        _rate: u32,
    ) -> Result<Box<dyn PluginInstance>, PluginHostError> {
        Err(PluginHostError::Unavailable)
    }

    fn forget_state(&mut self, _endpoint: u16) {}
}

/// The plugin host of this build: on Windows `ssr-plughost.exe` in
/// `exe_dir`, saving states in `state_dir`; elsewhere
/// [`UnavailablePluginHost`].
pub fn platform_plugin_host(exe_dir: &Path, state_dir: &Path) -> Box<dyn PluginHost + Send> {
    #[cfg(windows)]
    {
        Box::new(windows::WindowsPluginHost::new(
            paths::plughost_exe(exe_dir),
            state_dir.to_path_buf(),
        ))
    }
    #[cfg(not(windows))]
    {
        let _ = (exe_dir, state_dir);
        Box::new(UnavailablePluginHost)
    }
}

/// The speakers of a plugin bus, from the Windows channel-mask bits the host
/// reports in channel order. A bit no speaker has is refused with its value.
pub fn speakers_of(bits: &[u32]) -> Result<Vec<Speaker>, String> {
    bits.iter()
        .map(|&bit| Speaker::from_wave_bit(bit).ok_or_else(|| format!("speaker bit {bit:#x}")))
        .collect()
}

/// Refuses a host whose plugin has more than one active bus in a direction
/// (ADR 0039). Hosts start with `BusSelector::main_only()`, so this holds
/// unless a plugin reports more than it was given; a sidechain would then join
/// the upmixer's input speakers (a mono one as a centre) and an auxiliary
/// output its output speakers. The engine folds scenes onto the main buses.
pub fn only_main_buses(setup: &ProcessingSetup) -> Result<(), String> {
    let active = |buses: &[vst_audio_protocol::BusConfiguration]| {
        buses.iter().filter(|bus| bus.active).count()
    };
    let (inputs, outputs) = (active(&setup.inputs), active(&setup.outputs));
    if inputs > 1 || outputs > 1 {
        return Err(format!(
            "the plugin has {inputs} active input and {outputs} active output buses; \
             only one main bus each is hosted"
        ));
    }
    Ok(())
}

/// One endpoint's two saved plugin states (ADR 0021).
#[derive(Debug)]
pub struct StateStore {
    current: PathBuf,
    previous: PathBuf,
    /// The newest state on disk, by hash: the same state again is not
    /// saved, so it does not push the previous one out.
    saved: Option<u64>,
}

impl StateStore {
    pub fn new(state_dir: &Path, endpoint: u16) -> Self {
        Self {
            current: paths::upmix_state(state_dir, endpoint),
            previous: paths::upmix_previous_state(state_dir, endpoint),
            saved: None,
        }
    }

    /// The newest and the previous state, each when it can be read.
    pub fn load(&mut self) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        let read = |path: &Path| match fs::read(path) {
            Ok(blob) => Some(blob),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                log::line(&format!(
                    "plugin host: {} cannot be read: {error}",
                    path.display()
                ));
                None
            }
        };
        let current = read(&self.current);
        self.saved = current.as_deref().map(state_hash);
        (current, read(&self.previous))
    }

    /// Keeps `blob` as the newest state, the newest so far becoming the
    /// previous: written beside them, then renamed. A failure is logged.
    pub fn save(&mut self, blob: &[u8]) {
        let hash = state_hash(blob);
        if self.saved == Some(hash) {
            return;
        }
        match self.write(blob) {
            Ok(()) => self.saved = Some(hash),
            Err(error) => log::line(&format!(
                "plugin host: {} could not be saved: {error}",
                self.current.display()
            )),
        }
    }

    fn write(&self, blob: &[u8]) -> io::Result<()> {
        if let Some(dir) = self.current.parent() {
            fs::create_dir_all(dir)?;
        }
        let new = self.new_file();
        let mut file = fs::File::create(&new)?;
        file.write_all(blob)?;
        file.sync_all()?;
        drop(file);
        match fs::rename(&self.current, &self.previous) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        fs::rename(&new, &self.current)
    }

    /// Removes both states: another plugin was chosen.
    pub fn clear(&mut self) {
        for path in [&self.current, &self.previous, &self.new_file()] {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => log::line(&format!(
                    "plugin host: {} could not be removed: {error}",
                    path.display()
                )),
            }
        }
        self.saved = None;
    }

    fn new_file(&self) -> PathBuf {
        let mut name = self.current.clone().into_os_string();
        name.push(".new");
        PathBuf::from(name)
    }
}

fn state_hash(blob: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    blob.hash(&mut hasher);
    hasher.finish()
}

#[cfg(windows)]
mod windows {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use engine::Upmixer;
    use ssr_types::Speaker;
    use ssr_types::app::PluginFormat;
    use supervisor::upmix::{
        PluginClass, PluginEvent, PluginHost, PluginHostError, PluginInstance, PluginSetting,
    };
    use vst_audio::{
        BlockEndpoint, BlockStatus, BusSelector, Command, Configuration, Controller, Host,
        HostEvent, LoadTask, PluginDescriptor, RecoveryPolicy,
    };

    use super::{StateStore, only_main_buses, speakers_of};

    /// How long a stopping host is polled for its last state before it is
    /// dropped (which ends its process).
    const LEAVE_GRACE: Duration = Duration::from_secs(3);

    /// `ssr-plughost.exe` as the supervisor's plugin host port.
    #[derive(Clone, Debug)]
    pub struct WindowsPluginHost {
        host: Host,
        state_dir: PathBuf,
    }

    impl WindowsPluginHost {
        pub fn new(exe: PathBuf, state_dir: PathBuf) -> Self {
            Self {
                host: Host::new(exe),
                state_dir,
            }
        }
    }

    fn proto_format(format: PluginFormat) -> vst_audio::PluginFormat {
        match format {
            PluginFormat::Vst2 => vst_audio::PluginFormat::Vst2,
            PluginFormat::Vst3 => vst_audio::PluginFormat::Vst3,
        }
    }

    fn descriptor(plugin: &PluginSetting) -> PluginDescriptor {
        let mut descriptor = PluginDescriptor::new(&plugin.path, proto_format(plugin.format));
        descriptor.class_id = plugin.class;
        descriptor
    }

    fn configuration(rate: u32) -> Configuration {
        Configuration {
            sample_rate: rate,
            quantum: rate / 100,
            slots: Some(engine::UPMIX_RING_SLOTS),
            buses: Some(BusSelector::main_only()),
            recovery: RecoveryPolicy::Watch,
            ..Default::default()
        }
    }

    fn failed(error: impl std::fmt::Display) -> PluginHostError {
        PluginHostError::Failed(error.to_string())
    }

    impl PluginHost for WindowsPluginHost {
        fn scan(
            &mut self,
            path: &Path,
            format: PluginFormat,
        ) -> Result<Vec<PluginClass>, PluginHostError> {
            let classes = self
                .host
                .scan(&PluginDescriptor::new(path, proto_format(format)))
                .map_err(failed)?;
            Ok(classes
                .into_iter()
                .map(|class| PluginClass {
                    id: class.id,
                    name: class.name,
                    vendor: class.vendor,
                })
                .collect())
        }

        fn start(
            &mut self,
            endpoint: u16,
            plugin: &PluginSetting,
            rate: u32,
        ) -> Result<Box<dyn PluginInstance>, PluginHostError> {
            let mut instance = Instance {
                host: self.host.clone(),
                descriptor: descriptor(plugin),
                store: StateStore::new(&self.state_dir, endpoint),
                phase: Phase::Stopped,
                leaving: Vec::new(),
                stopping: false,
                exited: false,
            };
            let (current, previous) = instance.store.load();
            instance.begin_start(rate, current, previous);
            Ok(Box::new(instance))
        }

        fn forget_state(&mut self, endpoint: u16) {
            StateStore::new(&self.state_dir, endpoint).clear();
        }
    }

    enum Phase {
        Starting {
            task: LoadTask,
            rate: u32,
            previous: Option<Vec<u8>>,
        },
        /// The old host was asked for its state and to stop; the new one
        /// starts at `rate` once it is gone.
        Handoff {
            rate: u32,
        },
        Up(Box<Up>),
        /// Nothing runs: shut down, or a start failed.
        Stopped,
    }

    struct Up {
        instance: vst_audio::Instance,
        controller: Controller,
        inputs: Vec<Speaker>,
        outputs: Vec<Speaker>,
        latency: usize,
        upmixer: Option<Box<dyn Upmixer>>,
        fallback: bool,
    }

    /// The engine-facing realtime half of a high-level vst-audio instance.
    struct BlockUpmixer {
        endpoint: BlockEndpoint,
        inputs: Vec<Speaker>,
        outputs: Vec<Speaker>,
        latency: usize,
    }

    impl BlockUpmixer {
        fn new(endpoint: BlockEndpoint) -> Result<Self, String> {
            only_main_buses(endpoint.layout())?;
            let input = endpoint
                .layout()
                .inputs
                .iter()
                .find(|bus| bus.active)
                .ok_or_else(|| "the plugin has no active main input bus".to_owned())?;
            let output = endpoint
                .layout()
                .outputs
                .iter()
                .find(|bus| bus.active)
                .ok_or_else(|| "the plugin has no active main output bus".to_owned())?;
            let inputs = speakers_of(
                &vst_audio::wave_bits(input.layout.arrangement).map_err(|e| e.to_string())?,
            )?;
            let outputs = speakers_of(
                &vst_audio::wave_bits(output.layout.arrangement).map_err(|e| e.to_string())?,
            )?;
            let latency = endpoint.plugin_latency_frames() as usize;
            Ok(Self {
                endpoint,
                inputs,
                outputs,
                latency,
            })
        }
    }

    impl Upmixer for BlockUpmixer {
        fn rate(&self) -> u32 {
            self.endpoint.geometry().sample_rate
        }

        fn block_frames(&self) -> usize {
            self.endpoint.geometry().max_frames as usize
        }

        fn inputs(&self) -> &[Speaker] {
            &self.inputs
        }

        fn outputs(&self) -> &[Speaker] {
            &self.outputs
        }

        fn latency_frames(&self) -> usize {
            self.latency
        }

        fn set_depth(&mut self, depth: usize) {
            self.endpoint
                .restart(depth as u32)
                .expect("the stage checked the endpoint's maximum depth");
        }

        fn max_depth(&self) -> usize {
            self.endpoint.max_depth() as usize
        }

        fn process(
            &mut self,
            _block: u64,
            _generation: u64,
            input: &[f32],
            output: &mut [f32],
        ) -> bool {
            self.endpoint.process(input, output) == BlockStatus::Delivered
        }
    }

    struct Instance {
        host: Host,
        descriptor: PluginDescriptor,
        store: StateStore,
        phase: Phase,
        /// Instances asked to stop, polled for their last state until they exit.
        leaving: Vec<(vst_audio::Instance, Instant)>,
        stopping: bool,
        exited: bool,
    }

    impl Instance {
        fn begin_start(&mut self, rate: u32, state: Option<Vec<u8>>, previous: Option<Vec<u8>>) {
            let task = self
                .host
                .spawn_async(self.descriptor.clone(), configuration(rate), state);
            self.phase = Phase::Starting {
                task,
                rate,
                previous,
            };
        }

        fn up(&mut self, mut instance: vst_audio::Instance) -> Result<(), String> {
            let controller = instance.controller();
            let endpoint = instance
                .take_blocks()
                .ok_or_else(|| "the plugin instance has no block endpoint".to_owned())?;
            let upmixer = BlockUpmixer::new(endpoint)?;
            let inputs = upmixer.inputs.clone();
            let outputs = upmixer.outputs.clone();
            let latency = upmixer.latency;
            self.phase = Phase::Up(Box::new(Up {
                instance,
                controller,
                inputs,
                outputs,
                latency,
                upmixer: Some(Box::new(upmixer)),
                fallback: false,
            }));
            Ok(())
        }

        fn stop_instance(instance: &mut vst_audio::Instance) {
            let controller = instance.controller();
            let _ = controller.request(Command::SaveState);
            let _ = controller.request(Command::Shutdown);
        }

        /// Saves what stopping instances return and lets go of those that
        /// stopped or outstayed [`LEAVE_GRACE`].
        fn poll_leaving(&mut self) {
            let store = &mut self.store;
            self.leaving.retain_mut(|(instance, since)| {
                let mut stopped = false;
                while let Some(event) = instance.try_next_event() {
                    match event {
                        HostEvent::State { bytes, .. } => store.save(&bytes),
                        HostEvent::Stopped => stopped = true,
                        HostEvent::NotificationsLost => {
                            let _ = instance.controller().request(Command::SaveState);
                        }
                        _ => {}
                    }
                }
                !stopped && since.elapsed() < LEAVE_GRACE
            });
        }
    }

    impl PluginInstance for Instance {
        fn take_upmixer(&mut self) -> Option<Box<dyn Upmixer>> {
            match &mut self.phase {
                Phase::Up(up) => up.upmixer.take(),
                _ => None,
            }
        }

        fn inputs(&self) -> &[Speaker] {
            match &self.phase {
                Phase::Up(up) => &up.inputs,
                _ => &[],
            }
        }

        fn outputs(&self) -> &[Speaker] {
            match &self.phase {
                Phase::Up(up) => &up.outputs,
                _ => &[],
            }
        }

        fn latency_frames(&self) -> usize {
            match &self.phase {
                Phase::Up(up) => up.latency,
                _ => 0,
            }
        }

        /// `RecoveryPolicy::Watch` owns the restart clock and state choice.
        /// The port's `NeedsRespawn` variant remains for other adapters and tests,
        /// but this adapter never emits it.
        fn poll(&mut self, _now_ms: u64) -> Vec<PluginEvent> {
            let mut events = Vec::new();
            self.poll_leaving();

            let finished = match &self.phase {
                Phase::Starting { task, .. } => Some(task.try_complete()),
                _ => None,
            };
            if let Some(finished) = finished {
                match finished {
                    Ok(None) => {}
                    Ok(Some(instance)) => match self.up(instance) {
                        Ok(()) => events.push(PluginEvent::Running),
                        Err(detail) => {
                            self.phase = Phase::Stopped;
                            events.push(PluginEvent::StartFailed(detail));
                        }
                    },
                    Err(error) => {
                        let (rate, previous) =
                            match std::mem::replace(&mut self.phase, Phase::Stopped) {
                                Phase::Starting { rate, previous, .. } => (rate, previous),
                                _ => unreachable!("only a starting task can finish"),
                            };
                        if let Some(previous) = previous {
                            self.begin_start(rate, Some(previous), None);
                        } else {
                            events.push(PluginEvent::StartFailed(error.to_string()));
                        }
                    }
                }
            }

            if let Phase::Handoff { rate } = self.phase
                && self.leaving.is_empty()
            {
                let (current, previous) = self.store.load();
                self.begin_start(rate, current, previous);
            }

            if let Phase::Up(up) = &mut self.phase {
                while let Some(event) = up.instance.try_next_event() {
                    match event {
                        HostEvent::State { bytes, .. } => self.store.save(&bytes),
                        HostEvent::Failed { .. } => {
                            if !up.fallback {
                                up.fallback = true;
                                events.push(PluginEvent::Fallback(true));
                            }
                        }
                        HostEvent::Recovered { .. } => {
                            if up.fallback {
                                up.fallback = false;
                                events.push(PluginEvent::Fallback(false));
                            }
                        }
                        HostEvent::LatencyChanged(frames) if frames as usize != up.latency => {
                            events.push(PluginEvent::LatencyChanged(frames as usize));
                        }
                        HostEvent::RecoveryRequired => events.push(PluginEvent::Disabled),
                        HostEvent::NotificationsLost => {
                            let _ = up.controller.request(Command::SaveState);
                        }
                        HostEvent::Stopped if self.stopping => {
                            self.phase = Phase::Stopped;
                            break;
                        }
                        _ => {}
                    }
                }
            }

            if self.stopping
                && !self.exited
                && matches!(self.phase, Phase::Stopped)
                && self.leaving.is_empty()
            {
                self.exited = true;
                events.push(PluginEvent::Exited);
            }
            events
        }

        fn restart(&mut self, rate: u32) -> Result<(), PluginHostError> {
            match std::mem::replace(&mut self.phase, Phase::Stopped) {
                Phase::Up(mut up) => {
                    Self::stop_instance(&mut up.instance);
                    self.leaving.push((up.instance, Instant::now()));
                    self.phase = Phase::Handoff { rate };
                }
                Phase::Handoff { .. } => self.phase = Phase::Handoff { rate },
                Phase::Starting { .. } | Phase::Stopped => {
                    let (current, previous) = self.store.load();
                    self.begin_start(rate, current, previous);
                }
            }
            Ok(())
        }

        fn open_editor(&mut self) -> Result<(), PluginHostError> {
            match &self.phase {
                Phase::Up(up) => up
                    .controller
                    .request(Command::OpenEditor)
                    .map(drop)
                    .map_err(failed),
                _ => Err(PluginHostError::Failed(
                    "the plugin host is not up".to_owned(),
                )),
            }
        }

        fn shutdown(&mut self) {
            self.stopping = true;
            match std::mem::replace(&mut self.phase, Phase::Stopped) {
                Phase::Up(mut up) => {
                    Self::stop_instance(&mut up.instance);
                    self.leaving.push((up.instance, Instant::now()));
                }
                Phase::Starting { task, .. } => task.cancel(),
                Phase::Handoff { .. } | Phase::Stopped => {}
            }
        }
    }

    #[test]
    #[ignore = "needs ssr-plughost and vst-audio's test plugin"]
    fn windows_adapter_hosts_the_test_plugin_end_to_end() {
        let worker = PathBuf::from(std::env::var_os("VST_AUDIO_WORKER").expect("worker required"));
        let plugin_path =
            PathBuf::from(std::env::var_os("VST_AUDIO_TEST_VST3").expect("VST 3 fixture required"));
        let state_dir = std::env::temp_dir().join(format!(
            "ssr-windows-adapter-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut host = WindowsPluginHost::new(worker, state_dir.clone());
        let plugin = PluginSetting {
            path: plugin_path,
            format: PluginFormat::Vst3,
            class: None,
            name: "vst-audio test plugin".into(),
            vendor: "test fixture".into(),
            inputs: Vec::new(),
            outputs: Vec::new(),
        };
        let mut instance = host.start(0, &plugin, 48_000).unwrap();
        let mut deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if instance.poll(0).contains(&PluginEvent::Running) {
                break;
            }
            assert!(Instant::now() < deadline, "initial start timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut upmixer = instance.take_upmixer().expect("running upmixer");
        upmixer.set_depth(1);
        let input = vec![0.25; upmixer.inputs().len() * upmixer.block_frames()];
        let mut output = vec![0.0; upmixer.outputs().len() * upmixer.block_frames()];
        deadline = Instant::now() + Duration::from_secs(15);
        for block in 0.. {
            if upmixer.process(block, 0, &input, &mut output) {
                assert!((output[0] - 0.25).abs() < 1e-6);
                assert!((output[5 * upmixer.block_frames()] - 0.0625).abs() < 1e-6);
                break;
            }
            assert!(Instant::now() < deadline, "audio delivery timed out");
            std::thread::sleep(Duration::from_millis(3));
        }

        instance.restart(44_100).unwrap();
        deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if instance.poll(0).contains(&PluginEvent::Running) {
                assert_eq!(instance.take_upmixer().unwrap().rate(), 44_100);
                break;
            }
            assert!(Instant::now() < deadline, "restart timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
        instance.shutdown();
        deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if instance.poll(0).contains(&PluginEvent::Exited) {
                break;
            }
            assert!(Instant::now() < deadline, "shutdown timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(super::paths::upmix_state(&state_dir, 0).exists());
        let _ = std::fs::remove_dir_all(state_dir);
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_bits_map_to_speakers_and_unknown_bits_are_refused() {
        assert_eq!(
            speakers_of(&[0x1, 0x2, 0x4, 0x8, 0x10, 0x20]),
            Ok(vec![
                Speaker::L,
                Speaker::R,
                Speaker::C,
                Speaker::Lfe,
                Speaker::Lrs,
                Speaker::Rrs
            ])
        );
        assert_eq!(
            speakers_of(&[0x1, 0x40000]),
            Err("speaker bit 0x40000".into())
        );
    }

    #[test]
    fn only_main_buses_are_hosted() {
        use vst_audio_protocol::{Bus, BusConfiguration, SampleFormat};
        let bus = |channels: u16, arrangement: u64, active: bool| BusConfiguration {
            layout: Bus {
                channels,
                arrangement,
            },
            active,
        };
        let mut setup = ProcessingSetup {
            sample_rate: 48_000,
            quantum: 480,
            sample_format: SampleFormat::F32,
            offline: false,
            inputs: vec![bus(2, 0x3, true)],
            outputs: vec![bus(6, 0x3f, true)],
        };
        assert_eq!(only_main_buses(&setup), Ok(()));
        // A mono sidechain would otherwise become a centre input.
        setup.inputs.push(bus(1, 0x4, true));
        assert_eq!(
            only_main_buses(&setup),
            Err("the plugin has 2 active input and 1 active output buses; \
                 only one main bus each is hosted"
                .into())
        );
        // Declared but inactive buses carry no channels.
        setup.inputs[1].active = false;
        setup.outputs.push(bus(2, 0x3, false));
        assert_eq!(only_main_buses(&setup), Ok(()));
        setup.outputs[1].active = true;
        assert!(only_main_buses(&setup).is_err());
    }

    #[test]
    fn states_rotate_through_two_files_and_clear() {
        let dir = std::env::temp_dir().join(format!(
            "ssr-plugin-states-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut store = StateStore::new(&dir, 2);
        assert_eq!(store.load(), (None, None));
        store.save(b"one");
        assert_eq!(fs::read(dir.join("upmix-2.state")).unwrap(), b"one");
        assert!(!dir.join("upmix-2.previous.state").exists());
        store.save(b"two");
        // The same state again keeps the one before it.
        store.save(b"two");
        assert_eq!(store.load(), (Some(b"two".to_vec()), Some(b"one".to_vec())));
        store.save(b"three");
        assert_eq!(
            StateStore::new(&dir, 2).load(),
            (Some(b"three".to_vec()), Some(b"two".to_vec()))
        );
        assert_eq!(StateStore::new(&dir, 1).load(), (None, None));
        store.clear();
        assert_eq!(store.load(), (None, None));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hosting_is_unavailable_off_windows() {
        let mut plugins = UnavailablePluginHost;
        assert_eq!(
            plugins.scan(Path::new("Example.vst3"), PluginFormat::Vst3),
            Err(PluginHostError::Unavailable)
        );
        let plugin = PluginSetting {
            path: "Example.vst3".into(),
            format: PluginFormat::Vst3,
            class: None,
            name: "Example".into(),
            vendor: "Vendor".into(),
            inputs: Vec::new(),
            outputs: Vec::new(),
        };
        assert!(matches!(
            plugins.start(0, &plugin, 48_000),
            Err(PluginHostError::Unavailable)
        ));
    }
}
