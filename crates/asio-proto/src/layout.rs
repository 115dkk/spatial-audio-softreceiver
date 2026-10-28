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
    Tc,
    Tfc,
    Lbs,
    Rbs,
    Cs,
    Lfe2,
    Tsl,
    Tsr,
    Trc,
    Bfc,
    Bfl,
    Bfr,
    Lc,
    Rc,
    Lsc,
    Rsc,
    Ls1,
    Rs1,
    Lrs1,
    Rrs1,
    Lcs,
    Rcs,
    Hfl,
    Hfr,
    Hrl,
    Hrr,
    // The legacy 5.1 protocol names its 110-degree surrounds Ls/Rs. New layouts
    // use distinct channels with the same DAW-facing names so the legacy mapping
    // and its bytes stay unchanged.
    Ls110,
    Rs110,
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
            Self::Tc => "Tc",
            Self::Tfc => "Tfc",
            Self::Lbs => "Lbs",
            Self::Rbs => "Rbs",
            Self::Cs => "Cs",
            Self::Lfe2 => "LFE2",
            Self::Tsl => "Tsl",
            Self::Tsr => "Tsr",
            Self::Trc => "Trc",
            Self::Bfc => "Bfc",
            Self::Bfl => "Bfl",
            Self::Bfr => "Bfr",
            Self::Lc => "Lc",
            Self::Rc => "Rc",
            Self::Lsc => "Lsc",
            Self::Rsc => "Rsc",
            Self::Ls1 => "Ls1",
            Self::Rs1 => "Rs1",
            Self::Lrs1 => "Lrs1",
            Self::Rrs1 => "Rrs1",
            Self::Lcs => "Lcs",
            Self::Rcs => "Rcs",
            Self::Hfl => "Hfl",
            Self::Hfr => "Hfr",
            Self::Hrl => "Hrl",
            Self::Hrr => "Hrr",
            Self::Ls110 => "Ls",
            Self::Rs110 => "Rs",
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
const SURROUND_91: [Channel; 10] = [
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
];
const SURROUND_914: [Channel; 14] = [
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
    Channel::Trl,
    Channel::Trr,
];
const SURROUND_131: [Channel; 14] = [
    Channel::L,
    Channel::R,
    Channel::C,
    Channel::Lfe,
    Channel::Lrs,
    Channel::Rrs,
    Channel::Lss,
    Channel::Rss,
    Channel::Tc,
    Channel::Tfl,
    Channel::Tfc,
    Channel::Tfr,
    Channel::Trl,
    Channel::Trr,
];
const SURROUND_222: [Channel; 24] = [
    Channel::Lw,
    Channel::Rw,
    Channel::C,
    Channel::Lfe,
    Channel::Lbs,
    Channel::Rbs,
    Channel::L,
    Channel::R,
    Channel::Cs,
    Channel::Lfe2,
    Channel::Lss,
    Channel::Rss,
    Channel::Tfl,
    Channel::Tfr,
    Channel::Tfc,
    Channel::Tc,
    Channel::Trl,
    Channel::Trr,
    Channel::Tsl,
    Channel::Tsr,
    Channel::Trc,
    Channel::Bfc,
    Channel::Bfl,
    Channel::Bfr,
];
const SURROUND_24110: [Channel; 35] = [
    Channel::L,
    Channel::R,
    Channel::C,
    Channel::Lfe,
    Channel::Lss,
    Channel::Rss,
    Channel::Lbs,
    Channel::Rbs,
    Channel::Lw,
    Channel::Rw,
    Channel::Tfl,
    Channel::Tfr,
    Channel::Tsl,
    Channel::Tsr,
    Channel::Trl,
    Channel::Trr,
    Channel::Lc,
    Channel::Rc,
    Channel::Lsc,
    Channel::Rsc,
    Channel::Ls1,
    Channel::Rs1,
    Channel::Ls110,
    Channel::Rs110,
    Channel::Lrs1,
    Channel::Rrs1,
    Channel::Lrs,
    Channel::Rrs,
    Channel::Lcs,
    Channel::Rcs,
    Channel::Cs,
    Channel::Hfl,
    Channel::Hfr,
    Channel::Hrl,
    Channel::Hrr,
];
const SURROUND_302: [Channel; 32] = [
    Channel::C,
    Channel::L,
    Channel::R,
    Channel::Ls110,
    Channel::Rs110,
    Channel::Lfe,
    Channel::Cs,
    Channel::Lrs,
    Channel::Rrs,
    Channel::Lss,
    Channel::Rss,
    Channel::Lc,
    Channel::Rc,
    Channel::Hfl,
    Channel::Tfc,
    Channel::Hfr,
    Channel::Lfe2,
    Channel::Lw,
    Channel::Rw,
    Channel::Tc,
    Channel::Tsl,
    Channel::Tsr,
    Channel::Trc,
    Channel::Hrl,
    Channel::Hrr,
    Channel::Bfc,
    Channel::Bfl,
    Channel::Bfr,
    Channel::Tfl,
    Channel::Tfr,
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
const SURROUND_91_NAMES: [&str; 10] =
    ["L", "R", "C", "LFE", "Lrs", "Rrs", "Lss", "Rss", "Lw", "Rw"];
const SURROUND_914_NAMES: [&str; 14] = [
    "L", "R", "C", "LFE", "Lrs", "Rrs", "Lss", "Rss", "Lw", "Rw", "Tfl", "Tfr", "Trl", "Trr",
];
const SURROUND_131_NAMES: [&str; 14] = [
    "L", "R", "C", "LFE", "Lrs", "Rrs", "Lss", "Rss", "Tc", "Tfl", "Tfc", "Tfr", "Trl", "Trr",
];
const SURROUND_222_NAMES: [&str; 24] = [
    "Lw", "Rw", "C", "LFE", "Lbs", "Rbs", "L", "R", "Cs", "LFE2", "Lss", "Rss", "Tfl", "Tfr",
    "Tfc", "Tc", "Trl", "Trr", "Tsl", "Tsr", "Trc", "Bfc", "Bfl", "Bfr",
];
const SURROUND_24110_NAMES: [&str; 35] = [
    "L", "R", "C", "LFE", "Lss", "Rss", "Lbs", "Rbs", "Lw", "Rw", "Tfl", "Tfr", "Tsl", "Tsr",
    "Trl", "Trr", "Lc", "Rc", "Lsc", "Rsc", "Ls1", "Rs1", "Ls", "Rs", "Lrs1", "Rrs1", "Lrs", "Rrs",
    "Lcs", "Rcs", "Cs", "Hfl", "Hfr", "Hrl", "Hrr",
];
const SURROUND_302_NAMES: [&str; 32] = [
    "C", "L", "R", "Ls", "Rs", "LFE", "Cs", "Lrs", "Rrs", "Lss", "Rss", "Lc", "Rc", "Hfl", "Tfc",
    "Hfr", "LFE2", "Lw", "Rw", "Tc", "Tsl", "Tsr", "Trc", "Hrl", "Hrr", "Bfc", "Bfl", "Bfr", "Tfl",
    "Tfr", "Trl", "Trr",
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
    Surround91 = 5,
    Surround914 = 6,
    Surround131 = 7,
    Surround222 = 8,
    Surround24110 = 9,
    Surround302 = 10,
}

impl Layout {
    pub const fn from_wire(value: u32) -> Option<Self> {
        Some(match value {
            0 => Self::Stereo,
            1 => Self::Surround51,
            2 => Self::Surround71,
            3 => Self::Surround714,
            4 => Self::Surround916,
            5 => Self::Surround91,
            6 => Self::Surround914,
            7 => Self::Surround131,
            8 => Self::Surround222,
            9 => Self::Surround24110,
            10 => Self::Surround302,
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
            Self::Surround91 => &SURROUND_91,
            Self::Surround914 => &SURROUND_914,
            Self::Surround131 => &SURROUND_131,
            Self::Surround222 => &SURROUND_222,
            Self::Surround24110 => &SURROUND_24110,
            Self::Surround302 => &SURROUND_302,
        }
    }

    pub const fn channel_names(self) -> &'static [&'static str] {
        match self {
            Self::Stereo => &STEREO_NAMES,
            Self::Surround51 => &SURROUND_51_NAMES,
            Self::Surround71 => &SURROUND_71_NAMES,
            Self::Surround714 => &SURROUND_714_NAMES,
            Self::Surround916 => &SURROUND_916_NAMES,
            Self::Surround91 => &SURROUND_91_NAMES,
            Self::Surround914 => &SURROUND_914_NAMES,
            Self::Surround131 => &SURROUND_131_NAMES,
            Self::Surround222 => &SURROUND_222_NAMES,
            Self::Surround24110 => &SURROUND_24110_NAMES,
            Self::Surround302 => &SURROUND_302_NAMES,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Channel, Layout};

    #[test]
    fn all_layout_orders_and_names_are_fixed() {
        let cases: &[(u32, Layout, &[Channel], &[&str])] = &[
            (0, Layout::Stereo, &[Channel::L, Channel::R], &["L", "R"]),
            (
                1,
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
                2,
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
                3,
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
                4,
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
            (
                5,
                Layout::Surround91,
                &super::SURROUND_91,
                &["L", "R", "C", "LFE", "Lrs", "Rrs", "Lss", "Rss", "Lw", "Rw"],
            ),
            (
                6,
                Layout::Surround914,
                &super::SURROUND_914,
                &[
                    "L", "R", "C", "LFE", "Lrs", "Rrs", "Lss", "Rss", "Lw", "Rw", "Tfl", "Tfr",
                    "Trl", "Trr",
                ],
            ),
            (
                7,
                Layout::Surround131,
                &super::SURROUND_131,
                &[
                    "L", "R", "C", "LFE", "Lrs", "Rrs", "Lss", "Rss", "Tc", "Tfl", "Tfc", "Tfr",
                    "Trl", "Trr",
                ],
            ),
            (
                8,
                Layout::Surround222,
                &super::SURROUND_222,
                &[
                    "Lw", "Rw", "C", "LFE", "Lbs", "Rbs", "L", "R", "Cs", "LFE2", "Lss", "Rss",
                    "Tfl", "Tfr", "Tfc", "Tc", "Trl", "Trr", "Tsl", "Tsr", "Trc", "Bfc", "Bfl",
                    "Bfr",
                ],
            ),
            (
                9,
                Layout::Surround24110,
                &super::SURROUND_24110,
                &[
                    "L", "R", "C", "LFE", "Lss", "Rss", "Lbs", "Rbs", "Lw", "Rw", "Tfl", "Tfr",
                    "Tsl", "Tsr", "Trl", "Trr", "Lc", "Rc", "Lsc", "Rsc", "Ls1", "Rs1", "Ls", "Rs",
                    "Lrs1", "Rrs1", "Lrs", "Rrs", "Lcs", "Rcs", "Cs", "Hfl", "Hfr", "Hrl", "Hrr",
                ],
            ),
            (
                10,
                Layout::Surround302,
                &super::SURROUND_302,
                &[
                    "C", "L", "R", "Ls", "Rs", "LFE", "Cs", "Lrs", "Rrs", "Lss", "Rss", "Lc", "Rc",
                    "Hfl", "Tfc", "Hfr", "LFE2", "Lw", "Rw", "Tc", "Tsl", "Tsr", "Trc", "Hrl",
                    "Hrr", "Bfc", "Bfl", "Bfr", "Tfl", "Tfr", "Trl", "Trr",
                ],
            ),
        ];

        for &(wire, layout, order, names) in cases {
            assert_eq!(layout as u32, wire);
            assert_eq!(Layout::from_wire(wire), Some(layout));
            assert_eq!(layout.channels(), order.len() as u32);
            assert_eq!(layout.channel_order(), order);
            assert_eq!(layout.channel_names(), names);
            assert_eq!(
                order
                    .iter()
                    .map(|channel| channel.name())
                    .collect::<Vec<_>>(),
                names
            );
        }
        assert_eq!(Layout::from_wire(11), None);
        assert_eq!(Layout::default(), Layout::Surround714);
    }

    #[test]
    fn legacy_wire_values_zero_through_four_do_not_change() {
        assert_eq!(Layout::Stereo as u32, 0);
        assert_eq!(Layout::Surround51 as u32, 1);
        assert_eq!(Layout::Surround71 as u32, 2);
        assert_eq!(Layout::Surround714 as u32, 3);
        assert_eq!(Layout::Surround916 as u32, 4);
    }
}
