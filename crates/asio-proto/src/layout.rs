#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u32)]
pub enum Channel {
    L,
    R,
    C,
    Lfe,
    Ls,
    Rs,
    Lrs,
    Rrs,
    Lss,
    Rss,
    Lw,
    Rw,
    Tfl,
    Tfr,
    Tml,
    Tmr,
    Trl,
    Trr,
}

impl Channel {
    pub const fn name(self) -> &'static str {
        match self {
            Self::L => "L",
            Self::R => "R",
            Self::C => "C",
            Self::Lfe => "LFE",
            Self::Ls => "Ls",
            Self::Rs => "Rs",
            Self::Lrs => "Lrs",
            Self::Rrs => "Rrs",
            Self::Lss => "Lss",
            Self::Rss => "Rss",
            Self::Lw => "Lw",
            Self::Rw => "Rw",
            Self::Tfl => "Tfl",
            Self::Tfr => "Tfr",
            Self::Tml => "Tml",
            Self::Tmr => "Tmr",
            Self::Trl => "Trl",
            Self::Trr => "Trr",
        }
    }
}

const STEREO: [Channel; 2] = [Channel::L, Channel::R];
const SURROUND_51: [Channel; 6] = [
    Channel::L,
    Channel::R,
    Channel::C,
    Channel::Lfe,
    Channel::Ls,
    Channel::Rs,
];
const SURROUND_71: [Channel; 8] = [
    Channel::L,
    Channel::R,
    Channel::C,
    Channel::Lfe,
    Channel::Lrs,
    Channel::Rrs,
    Channel::Lss,
    Channel::Rss,
];
const SURROUND_714: [Channel; 12] = [
    Channel::L,
    Channel::R,
    Channel::C,
    Channel::Lfe,
    Channel::Lrs,
    Channel::Rrs,
    Channel::Lss,
    Channel::Rss,
    Channel::Tfl,
    Channel::Tfr,
    Channel::Trl,
    Channel::Trr,
];
const SURROUND_916: [Channel; 16] = [
    Channel::L,
    Channel::R,
    Channel::C,
    Channel::Lfe,
    Channel::Lrs,
    Channel::Rrs,
    Channel::Lss,
    Channel::Rss,
    Channel::Lw,
    Channel::Rw,
    Channel::Tfl,
    Channel::Tfr,
    Channel::Tml,
    Channel::Tmr,
    Channel::Trl,
    Channel::Trr,
];

const STEREO_NAMES: [&str; 2] = ["L", "R"];
const SURROUND_51_NAMES: [&str; 6] = ["L", "R", "C", "LFE", "Ls", "Rs"];
const SURROUND_71_NAMES: [&str; 8] = ["L", "R", "C", "LFE", "Lrs", "Rrs", "Lss", "Rss"];
const SURROUND_714_NAMES: [&str; 12] = [
    "L", "R", "C", "LFE", "Lrs", "Rrs", "Lss", "Rss", "Tfl", "Tfr", "Trl", "Trr",
];
const SURROUND_916_NAMES: [&str; 16] = [
    "L", "R", "C", "LFE", "Lrs", "Rrs", "Lss", "Rss", "Lw", "Rw", "Tfl", "Tfr", "Tml", "Tmr",
    "Trl", "Trr",
];

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(u32)]
pub enum Layout {
    Stereo = 0,
    Surround51 = 1,
    Surround71 = 2,
    #[default]
    Surround714 = 3,
    Surround916 = 4,
}

impl Layout {
    pub const fn from_wire(value: u32) -> Option<Self> {
        Some(match value {
            0 => Self::Stereo,
            1 => Self::Surround51,
            2 => Self::Surround71,
            3 => Self::Surround714,
            4 => Self::Surround916,
            _ => return None,
        })
    }

    pub const fn channels(self) -> u32 {
        self.channel_order().len() as u32
    }

    pub const fn channel_order(self) -> &'static [Channel] {
        match self {
            Self::Stereo => &STEREO,
            Self::Surround51 => &SURROUND_51,
            Self::Surround71 => &SURROUND_71,
            Self::Surround714 => &SURROUND_714,
            Self::Surround916 => &SURROUND_916,
        }
    }

    pub const fn channel_names(self) -> &'static [&'static str] {
        match self {
            Self::Stereo => &STEREO_NAMES,
            Self::Surround51 => &SURROUND_51_NAMES,
            Self::Surround71 => &SURROUND_71_NAMES,
            Self::Surround714 => &SURROUND_714_NAMES,
            Self::Surround916 => &SURROUND_916_NAMES,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Channel, Layout};

    #[test]
    fn all_layout_orders_and_names_are_fixed() {
        let cases: &[(Layout, &[Channel], &[&str])] = &[
            (Layout::Stereo, &[Channel::L, Channel::R], &["L", "R"]),
            (
                Layout::Surround51,
                &[
                    Channel::L,
                    Channel::R,
                    Channel::C,
                    Channel::Lfe,
                    Channel::Ls,
                    Channel::Rs,
                ],
                &["L", "R", "C", "LFE", "Ls", "Rs"],
            ),
            (
                Layout::Surround71,
                &[
                    Channel::L,
                    Channel::R,
                    Channel::C,
                    Channel::Lfe,
                    Channel::Lrs,
                    Channel::Rrs,
                    Channel::Lss,
                    Channel::Rss,
                ],
                &["L", "R", "C", "LFE", "Lrs", "Rrs", "Lss", "Rss"],
            ),
            (
                Layout::Surround714,
                &[
                    Channel::L,
                    Channel::R,
                    Channel::C,
                    Channel::Lfe,
                    Channel::Lrs,
                    Channel::Rrs,
                    Channel::Lss,
                    Channel::Rss,
                    Channel::Tfl,
                    Channel::Tfr,
                    Channel::Trl,
                    Channel::Trr,
                ],
                &[
                    "L", "R", "C", "LFE", "Lrs", "Rrs", "Lss", "Rss", "Tfl", "Tfr", "Trl", "Trr",
                ],
            ),
            (
                Layout::Surround916,
                &[
                    Channel::L,
                    Channel::R,
                    Channel::C,
                    Channel::Lfe,
                    Channel::Lrs,
                    Channel::Rrs,
                    Channel::Lss,
                    Channel::Rss,
                    Channel::Lw,
                    Channel::Rw,
                    Channel::Tfl,
                    Channel::Tfr,
                    Channel::Tml,
                    Channel::Tmr,
                    Channel::Trl,
                    Channel::Trr,
                ],
                &[
                    "L", "R", "C", "LFE", "Lrs", "Rrs", "Lss", "Rss", "Lw", "Rw", "Tfl", "Tfr",
                    "Tml", "Tmr", "Trl", "Trr",
                ],
            ),
        ];

        for (wire, (layout, order, names)) in cases.iter().enumerate() {
            assert_eq!(Layout::from_wire(wire as u32), Some(*layout));
            assert_eq!(layout.channels(), order.len() as u32);
            assert_eq!(layout.channel_order(), *order);
            assert_eq!(layout.channel_names(), *names);
            assert_eq!(
                order
                    .iter()
                    .map(|channel| channel.name())
                    .collect::<Vec<_>>(),
                *names
            );
        }
        assert_eq!(Layout::from_wire(5), None);
        assert_eq!(Layout::default(), Layout::Surround714);
    }
}
