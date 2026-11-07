//! Cooling fins on a face of the box: a set of equal plates that rise outwards from the face, side
//! by side, and an estimate of the heat they give to still air.
//!
//! A [`FinSet`] is a handful of numbers (how many, how thick, how tall, how far apart, where the
//! first one is, where they start and how long they run), never a mesh. Its footprints are
//! rectangles on the face, in integer nanometres doubled so that half a thickness is exact; the
//! checks ([`FinSet::check`]) compare them with the edges of the face, the holes of the face and
//! the other sets on it. The limits (the most fins, the thinnest, the least air between two, the
//! tallest for their thickness) are settings, not constants.
//!
//! [`FinSet::thermal`] is an **estimate** of a simple model: one coefficient of heat transfer for
//! the whole surface (natural convection in still air, from the settings), fins of constant
//! section whose tip is counted as if it were as hot as the air, no radiation and no interference
//! between neighbouring fins. It says how much more heat a set of fins gives to the air than the
//! same strip of the face without them, and what that is for a base that is warmer than the air by
//! the amount of the settings. It is not a simulation and has been compared with no measurement.

use crate::config::{DesignLimits, ThermalSettings};
use crate::enclosure::{Face, Hole};
use crate::units::Count;
use crate::{CadError, Length};

/// The direction the fins run along the face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinAxis {
    /// The fins run along `u` and are spaced along `v`.
    U,
    /// The fins run along `v` and are spaced along `u`.
    V,
}

impl FinAxis {
    /// A stable lower-case name: `u` or `v`.
    pub fn name(self) -> &'static str {
        match self {
            FinAxis::U => "u",
            FinAxis::V => "v",
        }
    }

    /// The axis with a name given by [`FinAxis::name`], or `None`.
    pub fn from_name(name: &str) -> Option<FinAxis> {
        match name {
            "u" => Some(FinAxis::U),
            "v" => Some(FinAxis::V),
            _ => None,
        }
    }
}

/// A set of equal cooling fins on a face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinSet {
    /// The name shown to people; an attribute, it never identifies the set.
    pub name: String,
    /// The face the fins rise from, outwards.
    pub face: Face,
    /// The direction the fins run.
    pub along: FinAxis,
    /// How many fins.
    pub count: Count,
    /// The thickness of one fin.
    pub thickness: Length,
    /// How far a fin rises from the face.
    pub height: Length,
    /// The distance from the middle of one fin to the middle of the next.
    pub pitch: Length,
    /// Where the middle of the first fin is, across the direction they run, from the lower left
    /// corner of the face.
    pub first: Length,
    /// Where the fins start, along the direction they run, from the lower left corner of the face.
    pub start: Length,
    /// How long each fin runs.
    pub length: Length,
}

/// A rectangle on a face, in nanometres doubled (so that the middle of a thickness is whole).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect2 {
    /// The smallest `u`, doubled.
    pub u0: i128,
    /// The largest `u`, doubled.
    pub u1: i128,
    /// The smallest `v`, doubled.
    pub v0: i128,
    /// The largest `v`, doubled.
    pub v1: i128,
}

impl Rect2 {
    /// Whether the two rectangles are nearer than `clearance` (doubled too), on both axes at once.
    pub fn nearer_than(&self, other: &Rect2, clearance: i128) -> bool {
        self.u0 - clearance < other.u1
            && other.u0 - clearance < self.u1
            && self.v0 - clearance < other.v1
            && other.v0 - clearance < self.v1
    }
}

/// What the checks of a set need to know about the box and the face, in nanometres.
#[derive(Debug, Clone, Copy)]
pub struct FinContext<'a> {
    /// The limits of the settings.
    pub limits: DesignLimits,
    /// The size of the face along `u`.
    pub width: i128,
    /// The size of the face along `v`.
    pub height: i128,
    /// The least distance from the fins to the four edges of the face: at the smallest `u`, at
    /// the largest `u`, at the smallest `v` and at the largest `v`.
    pub margins: [i128; 4],
    /// The least material between a fin and a hole.
    pub min_web: i128,
    /// The holes of the face (the holes of the lid fixing included).
    pub holes: &'a [Hole],
    /// The other sets on the same face.
    pub others: &'a [&'a FinSet],
}

/// The result of the estimate of the heat that a set of fins gives to still air.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thermal {
    /// The surface of the fins and of the bare face between them, in square millimetres.
    pub area_mm2: f64,
    /// The surface of the fins alone, in square millimetres.
    pub fin_area_mm2: f64,
    /// The bare face between the fins, in square millimetres.
    pub base_area_mm2: f64,
    /// The fraction of the heat a fin would give if it were as warm along its whole height as at
    /// its foot (from 0 to 1).
    pub efficiency: f64,
    /// The heat given to the air, in watts, for the temperature of the settings.
    pub power_w: f64,
    /// The heat the same strip of the face gives without fins, in watts.
    pub flat_power_w: f64,
    /// The thermal resistance from the base to the air, in kelvin per watt.
    pub resistance_k_w: f64,
}

fn bad(why: impl Into<String>) -> CadError {
    CadError::Fins(why.into())
}

/// Nanometres as millimetres, for the text of an error.
fn mm_of(nm: i128) -> String {
    let sign = if nm < 0 { "-" } else { "" };
    let a = nm.unsigned_abs();
    let (int, frac) = (a / 1_000_000, a % 1_000_000);
    if frac == 0 {
        format!("{sign}{int} mm")
    } else {
        let digits = format!("{frac:06}");
        format!("{sign}{int}.{} mm", digits.trim_end_matches('0'))
    }
}

impl FinSet {
    /// The footprint of each fin on the face, in order, as doubled nanometres.
    pub fn rects(&self) -> Vec<Rect2> {
        let t = i128::from(self.thickness.get());
        let pitch = i128::from(self.pitch.get());
        let first = i128::from(self.first.get());
        let start = i128::from(self.start.get());
        let length = i128::from(self.length.get());
        (0..i128::from(self.count.get()))
            .map(|k| {
                let middle2 = 2 * first + 2 * k * pitch;
                let (across0, across1) = (middle2 - t, middle2 + t);
                let (along0, along1) = (2 * start, 2 * (start + length));
                match self.along {
                    FinAxis::U => Rect2 {
                        u0: along0,
                        u1: along1,
                        v0: across0,
                        v1: across1,
                    },
                    FinAxis::V => Rect2 {
                        u0: across0,
                        u1: across1,
                        v0: along0,
                        v1: along1,
                    },
                }
            })
            .collect()
    }

    /// The smallest rectangle that holds every fin, as doubled nanometres.
    pub fn bounds(&self) -> Rect2 {
        let rects = self.rects();
        let mut b = rects.first().copied().unwrap_or(Rect2 {
            u0: 0,
            u1: 0,
            v0: 0,
            v1: 0,
        });
        for r in &rects {
            b.u0 = b.u0.min(r.u0);
            b.u1 = b.u1.max(r.u1);
            b.v0 = b.v0.min(r.v0);
            b.v1 = b.v1.max(r.v1);
        }
        b
    }

    /// The volume of material the fins add, in cubic nanometres: exact, because the fins are
    /// boxes that do not touch.
    pub fn volume_nm3(&self) -> i128 {
        i128::from(self.count.get())
            * i128::from(self.thickness.get())
            * i128::from(self.height.get())
            * i128::from(self.length.get())
    }

    /// The checks of the set against the box: the count and the shape of the fins against the
    /// limits of the settings, the air between the fins, the edges of the face (with the margin
    /// and the treatment of the edges), the holes of the face and the other sets on it. The first
    /// thing wrong is told, with the name of the set, as `PHOENIX-CAD-043`.
    pub fn check(&self, ctx: &FinContext<'_>) -> Result<(), CadError> {
        let name = &self.name;
        let limits = ctx.limits;
        let n = self.count.get();
        if n == 0 {
            return Err(bad(format!("{name}: a set has at least one fin")));
        }
        if n > limits.fin_max_count {
            return Err(bad(format!(
                "{name}: {n} fins are more than the {} that the settings allow",
                limits.fin_max_count
            )));
        }
        let t = i128::from(self.thickness.get());
        let height = i128::from(self.height.get());
        let min_t = i128::from(limits.fin_min_thickness.get());
        if t < min_t {
            return Err(bad(format!(
                "{name}: the thickness {} is under the least of {}",
                mm_of(t),
                mm_of(min_t)
            )));
        }
        if height <= 0 {
            return Err(bad(format!("{name}: the height is not positive")));
        }
        if height > t * i128::from(limits.fin_max_ratio) {
            return Err(bad(format!(
                "{name}: the height {} is more than {} times the thickness {}",
                mm_of(height),
                limits.fin_max_ratio,
                mm_of(t)
            )));
        }
        if self.length.get() <= 0 {
            return Err(bad(format!("{name}: the length is not positive")));
        }
        if n >= 2 {
            let pitch = i128::from(self.pitch.get());
            let gap = pitch - t;
            let min_gap = i128::from(limits.fin_min_gap.get());
            if gap < min_gap {
                return Err(bad(format!(
                    "{name}: the air between two fins is {}, under the least of {}",
                    mm_of(gap),
                    mm_of(min_gap)
                )));
            }
        }
        let rects = self.rects();
        let b = self.bounds();
        let [m_u0, m_u1, m_v0, m_v1] = ctx.margins;
        if b.u0 < 2 * m_u0
            || b.u1 > 2 * (ctx.width - m_u1)
            || b.v0 < 2 * m_v0
            || b.v1 > 2 * (ctx.height - m_v1)
        {
            return Err(bad(format!(
                "{name}: the fins do not lie inside the face {} with the margin of its edges",
                self.face.name()
            )));
        }
        let web2 = 2 * ctx.min_web;
        for hole in ctx.holes {
            let (su, sv) = hole.style.footprint(hole.diameter);
            let (u, v) = (i128::from(hole.u.get()), i128::from(hole.v.get()));
            let around = Rect2 {
                u0: 2 * u - su,
                u1: 2 * u + su,
                v0: 2 * v - sv,
                v1: 2 * v + sv,
            };
            if rects.iter().any(|r| r.nearer_than(&around, web2)) {
                return Err(bad(format!(
                    "{name}: the fins meet the hole {} (or leave less than the least web)",
                    hole.name
                )));
            }
        }
        let gap2 = 2 * i128::from(limits.fin_min_gap.get());
        for other in ctx.others {
            let theirs = other.rects();
            if rects
                .iter()
                .any(|r| theirs.iter().any(|o| r.nearer_than(o, gap2)))
            {
                return Err(bad(format!(
                    "{name}: the fins are too near the set {}",
                    other.name
                )));
            }
        }
        Ok(())
    }

    /// The estimate of the heat that the set gives to still air (see the header of the module).
    ///
    /// With `A` the surface of a fin (two sides, the tip and the two ends), `A_b` the bare face
    /// between the fins, `h` the coefficient of the settings, `k` the conductivity, `t` the
    /// thickness and `H` the height: `m = sqrt(2 h / (k t))`, the efficiency of a fin is
    /// `tanh(m L) / (m L)` for `L = H + t / 2` and the heat is `h dT (A_b + efficiency n A)`.
    pub fn thermal(&self, settings: &ThermalSettings) -> Thermal {
        const NM2_PER_MM2: f64 = 1_000_000_000_000.0;
        let n = i128::from(self.count.get());
        let t = i128::from(self.thickness.get());
        let height = i128::from(self.height.get());
        let length = i128::from(self.length.get());
        let pitch = i128::from(self.pitch.get());
        let fin_nm2 = 2 * height * length + t * length + 2 * height * t;
        let span = (n - 1) * pitch + t;
        let base_nm2 = (span * length - n * t * length).max(0);
        let fin_area_mm2 = (n * fin_nm2) as f64 / NM2_PER_MM2;
        let base_area_mm2 = base_nm2 as f64 / NM2_PER_MM2;
        let h = settings.convection_w_m2k();
        let k = settings.conductivity_w_mk();
        let d_t = settings.delta_t_k();
        let t_m = t as f64 * 1e-9;
        let lc = (height as f64 + t as f64 / 2.0) * 1e-9;
        let ml = (2.0 * h / (k * t_m)).sqrt() * lc;
        let efficiency = if ml > 0.0 { ml.tanh() / ml } else { 1.0 };
        let to_m2 = 1e-6;
        let power_w = h * d_t * (base_area_mm2 * to_m2 + efficiency * fin_area_mm2 * to_m2);
        let flat_power_w = h * d_t * (span * length) as f64 / NM2_PER_MM2 * to_m2;
        Thermal {
            area_mm2: fin_area_mm2 + base_area_mm2,
            fin_area_mm2,
            base_area_mm2,
            efficiency,
            power_w,
            flat_power_w,
            resistance_k_w: d_t / power_w,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::enclosure::HoleStyle;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn fins() -> FinSet {
        FinSet {
            name: "sink".into(),
            face: Face::Top,
            along: FinAxis::U,
            count: Count::new(3).unwrap(),
            thickness: mm(2),
            height: mm(10),
            pitch: mm(8),
            first: mm(20),
            start: mm(10),
            length: mm(50),
        }
    }

    fn context<'a>(holes: &'a [Hole], others: &'a [&'a FinSet]) -> FinContext<'a> {
        FinContext {
            limits: Settings::defaults().design,
            width: 104_000_000,
            height: 64_000_000,
            margins: [3_000_000; 4],
            min_web: 2_000_000,
            holes,
            others,
        }
    }

    fn code(r: Result<(), CadError>) -> String {
        match r {
            Ok(()) => "ok".into(),
            Err(e) => format!("{} {e}", e.code()),
        }
    }

    #[test]
    fn the_footprints_are_boxes_along_the_axis_and_exact_for_half_thicknesses() {
        // three fins 2 mm thick, 8 mm apart from 20 mm, along u from 10 to 60 mm: the middles
        // are at 20, 28 and 36 mm, so across they cover 19-21, 27-29 and 35-37 mm (doubled)
        let r = fins().rects();
        let doubled: Vec<(i128, i128, i128, i128)> =
            r.iter().map(|r| (r.u0, r.u1, r.v0, r.v1)).collect();
        assert_eq!(
            doubled,
            vec![
                (20_000_000, 120_000_000, 38_000_000, 42_000_000),
                (20_000_000, 120_000_000, 54_000_000, 58_000_000),
                (20_000_000, 120_000_000, 70_000_000, 74_000_000),
            ]
        );
        // along v the two axes swap
        let mut v = fins();
        v.along = FinAxis::V;
        let r = v.rects();
        assert_eq!(
            (r[2].u0, r[2].u1, r[2].v0, r[2].v1),
            (70_000_000, 74_000_000, 20_000_000, 120_000_000)
        );
        // an odd thickness has a half nanometre for the middle: 3 nm thick, middle at 5 nm
        let mut odd = fins();
        odd.thickness = Length::new(3).unwrap();
        odd.first = Length::new(5).unwrap();
        odd.count = Count::new(1).unwrap();
        assert_eq!((odd.rects()[0].v0, odd.rects()[0].v1), (7, 13));
        let b = fins().bounds();
        assert_eq!(
            (b.u0, b.u1, b.v0, b.v1),
            (20_000_000, 120_000_000, 38_000_000, 74_000_000)
        );
    }

    #[test]
    fn the_volume_is_the_boxes_of_the_fins() {
        // 3 fins of 2 x 10 x 50 mm = 3000 mm3
        assert_eq!(fins().volume_nm3(), 3_000i128 * 1_000_000_000_000_000_000);
    }

    #[test]
    fn a_good_set_passes_and_each_limit_has_its_own_reason() {
        assert_eq!(code(fins().check(&context(&[], &[]))), "ok");
        let with = |f: &dyn Fn(&mut FinSet)| {
            let mut s = fins();
            f(&mut s);
            code(s.check(&context(&[], &[])))
        };
        // the least air is 3 mm: a pitch of 5 mm leaves 3 mm, 4.999999 mm leaves less
        assert_eq!(with(&|s| s.pitch = mm(5)), "ok");
        let e = with(&|s| s.pitch = Length::new(4_999_999).unwrap());
        assert!(
            e.starts_with("PHOENIX-CAD-043") && e.contains("air between two fins is 2.999999 mm"),
            "{e}"
        );
        // a single fin has no neighbour: its pitch is not looked at
        assert_eq!(
            with(&|s| {
                s.count = Count::new(1).unwrap();
                s.pitch = mm(1);
            }),
            "ok"
        );
        // the thinnest is 0.8 mm
        assert_eq!(
            with(&|s| {
                s.thickness = Length::new(800_000).unwrap();
                s.height = mm(8)
            }),
            "ok"
        );
        let e = with(&|s| s.thickness = Length::new(799_999).unwrap());
        assert!(
            e.contains("thickness 0.799999 mm is under the least of 0.8 mm"),
            "{e}"
        );
        // the tallest is 12 times the thickness: 24 mm for 2 mm
        assert_eq!(with(&|s| s.height = mm(24)), "ok");
        let e = with(&|s| s.height = Length::new(24_000_001).unwrap());
        assert!(e.contains("more than 12 times the thickness 2 mm"), "{e}");
        assert!(with(&|s| s.height = Length::ZERO).contains("height is not positive"));
        assert!(with(&|s| s.length = Length::ZERO).contains("length is not positive"));
        assert!(with(&|s| s.count = Count::new(0).unwrap()).contains("at least one fin"));
        // the most is 128
        let e = with(&|s| s.count = Count::new(129).unwrap());
        assert!(e.contains("129 fins are more than the 128"), "{e}");
    }

    #[test]
    fn the_fins_keep_the_margin_of_the_edges_on_every_side() {
        let with = |f: &dyn Fn(&mut FinSet)| {
            let mut s = fins();
            f(&mut s);
            code(s.check(&context(&[], &[])))
        };
        // along u: from 3 to 101 mm (the face is 104 mm wide); across: the first fin at 4 mm has
        // its edge at 3 mm
        assert_eq!(
            with(&|s| {
                s.start = mm(3);
                s.length = mm(98)
            }),
            "ok"
        );
        assert!(with(&|s| {
            s.start = Length::new(2_999_999).unwrap();
            s.length = mm(50)
        })
        .contains("do not lie inside"));
        assert!(with(&|s| {
            s.start = mm(3);
            s.length = Length::new(98_000_001).unwrap()
        })
        .contains("do not lie inside"));
        assert_eq!(with(&|s| s.first = mm(4)), "ok");
        assert!(with(&|s| s.first = Length::new(3_999_999).unwrap()).contains("do not lie inside"));
        // the last fin's middle is first + 2 pitches: 64 - 3 - 1 = 60 mm is the most
        assert_eq!(with(&|s| s.first = mm(44)), "ok");
        assert!(with(&|s| s.first = Length::new(44_000_001).unwrap()).contains("do not lie inside"));
        // along v the face is 64 mm tall and 104 wide: the same set turned runs out of room
        assert!(with(&|s| {
            s.along = FinAxis::V;
            s.start = mm(10);
            s.length = mm(55)
        })
        .contains("do not lie inside"));
        assert_eq!(
            with(&|s| {
                s.along = FinAxis::V;
                s.start = mm(10);
                s.length = mm(51)
            }),
            "ok"
        );
    }

    #[test]
    fn a_hole_and_another_set_must_be_clear_of_the_fins() {
        let round = |u: i64, v: i64, d: i64| Hole {
            name: "h".into(),
            face: Face::Top,
            u: mm(u),
            v: mm(v),
            diameter: mm(d),
            style: HoleStyle::Plain,
        };
        // the fins cover u 10-60 and v 19-37; a 6 mm hole at v 43 spans 40-46: 3 mm clear
        assert_eq!(code(fins().check(&context(&[round(30, 43, 6)], &[]))), "ok");
        // at v 41 it spans 38-44: 1 mm clear, less than the web of 2 mm
        let e = code(fins().check(&context(&[round(30, 41, 6)], &[])));
        assert!(e.contains("meet the hole h"), "{e}");
        // beyond the end of the fins in u: the hole at u 66 spans 63-69, 3 mm clear
        assert_eq!(code(fins().check(&context(&[round(66, 30, 6)], &[]))), "ok");
        assert!(code(fins().check(&context(&[round(64, 30, 6)], &[]))).contains("meet the hole"));
        // a countersunk hole counts by the width of its head: 10 mm at u 70 spans 65-75, 5 mm clear
        let mut sunk = round(70, 30, 4);
        sunk.style = HoleStyle::Countersunk {
            head_diameter: mm(10),
        };
        assert_eq!(code(fins().check(&context(&[sunk.clone()], &[]))), "ok");
        sunk.u = mm(66);
        assert!(code(fins().check(&context(&[sunk], &[]))).contains("meet the hole"));
        // another set: 3 mm of air between them is the least
        let mut other = fins();
        other.name = "other".into();
        other.first = mm(40); // its first middle at 40 mm, edge at 39 mm; ours end at 37 mm
        assert!(code(fins().check(&context(&[], &[&other]))).contains("too near the set other"));
        other.first = mm(41); // edge at 40 mm: exactly 3 mm of air
        assert_eq!(code(fins().check(&context(&[], &[&other]))), "ok");
    }

    #[test]
    fn the_estimate_of_ten_fins_of_aluminium_in_still_air_is_worked_out_by_hand() {
        // 10 fins 2 x 20 mm, 60 mm long, 6 mm apart; h = 8 W/m2K, k = 200 W/mK, 40 K
        // one fin: 2 * 20 * 60 + 2 * 60 + 2 * 20 * 2 = 2600 mm2; the bare face is
        // 56 * 60 - 10 * 2 * 60 = 2160 mm2; the total is 28160 mm2
        // m = sqrt(2 * 8 / (200 * 0.002)) = sqrt(40); m L = sqrt(40) * 0.021 = 0.13283
        let mut set = fins();
        set.count = Count::new(10).unwrap();
        set.height = mm(20);
        set.pitch = mm(6);
        set.length = mm(60);
        let t = set.thermal(&Settings::defaults().thermal);
        assert_eq!(t.fin_area_mm2, 26_000.0);
        assert_eq!(t.base_area_mm2, 2_160.0);
        assert_eq!(t.area_mm2, 28_160.0);
        assert!(
            (t.efficiency - 0.994_161_195_149_056_1).abs() < 1e-12,
            "{}",
            t.efficiency
        );
        assert!(
            (t.power_w - 8.962_621_143_640_146).abs() < 1e-9,
            "{}",
            t.power_w
        );
        assert!(
            (t.flat_power_w - 1.0752).abs() < 1e-12,
            "{}",
            t.flat_power_w
        );
        assert!((t.resistance_k_w - 4.462_980_121_432_881).abs() < 1e-9);
        // the same fins printed in plastic (0.2 W/mK) are much less use: the heat does not climb
        let plastic = ThermalSettings {
            conductivity_mw_mk: 200,
            ..Settings::defaults().thermal
        };
        let p = set.thermal(&plastic);
        assert!(
            (p.efficiency - 0.237_988_182_490_365_1).abs() < 1e-12,
            "{}",
            p.efficiency
        );
        assert!(
            (p.power_w - 2.671_261_678_319_837).abs() < 1e-9,
            "{}",
            p.power_w
        );
        assert!(p.power_w < t.power_w && p.power_w > p.flat_power_w);
    }

    #[test]
    fn one_fin_has_no_bare_face_and_the_settings_scale_the_heat() {
        // one fin 3 x 10 mm, 40 mm long: 2 * 10 * 40 + 3 * 40 + 2 * 10 * 3 = 980 mm2, no bare
        // face between fins; 5 W/m2K, 200 W/mK, 30 K
        let mut one = fins();
        one.count = Count::new(1).unwrap();
        one.thickness = mm(3);
        one.height = mm(10);
        one.length = mm(40);
        let settings = ThermalSettings {
            convection_mw_m2k: 5_000,
            conductivity_mw_mk: 200_000,
            delta_t_mk: 30_000,
        };
        let t = one.thermal(&settings);
        assert_eq!(
            (t.fin_area_mm2, t.base_area_mm2, t.area_mm2),
            (980.0, 0.0, 980.0)
        );
        assert!((t.efficiency - 0.999_265_924_980_461).abs() < 1e-12);
        assert!(
            (t.power_w - 0.146_892_090_972_127_12).abs() < 1e-10,
            "{}",
            t.power_w
        );
        assert!((t.flat_power_w - 0.018).abs() < 1e-12);
        // twice the temperature difference is twice the heat, and the coefficient too
        let hot = ThermalSettings {
            delta_t_mk: 60_000,
            ..settings
        };
        assert!((one.thermal(&hot).power_w - 2.0 * t.power_w).abs() < 1e-12);
    }

    // ---- the fins in the enclosure --------------------------------------------------------

    use crate::enclosure::{Edging, Enclosure, EnclosureParams, Id, Operation, Treatment};
    use crate::mount::{Lid, LidFixing};
    use eh3d_container::ObjectId;

    fn enclosure() -> Enclosure {
        Enclosure::new(EnclosureParams {
            inner_length: mm(100),
            inner_width: mm(60),
            inner_height: mm(30),
            wall: mm(2),
            floor: mm(2),
            lid: mm(3),
            edge_margin: mm(3),
            min_web: mm(2),
        })
        .unwrap()
    }

    fn fid(n: u64) -> Id {
        ObjectId::from_parts(1_700_000_000_000, n as u16, n).unwrap()
    }

    fn add_fins(id: u64, set: FinSet) -> Operation {
        Operation::AddFins {
            id: fid(id),
            fins: set,
        }
    }

    fn round_hole(name: &str, u: i64, v: i64) -> Hole {
        Hole {
            name: name.into(),
            face: Face::Top,
            u: mm(u),
            v: mm(v),
            diameter: mm(6),
            style: HoleStyle::Plain,
        }
    }

    #[test]
    fn a_set_of_fins_is_added_removed_and_counted_in_the_volume() {
        let mut e = enclosure();
        let before = e.clone();
        let undo = e.apply(add_fins(1, fins())).unwrap();
        assert_eq!(undo, Operation::RemoveFins { id: fid(1) });
        assert_eq!(e.fins().len(), 1);
        // 3 fins of 2 x 10 x 50 mm add 3000 mm3 and the pockets take nothing
        assert_eq!(
            e.features_volume_nm3(),
            (3_000i128 * 1_000_000_000_000_000_000, 0)
        );
        let redo = e.apply(undo).unwrap();
        assert_eq!(e, before);
        assert_eq!(redo, add_fins(1, fins()));
        e.apply(redo).unwrap();
        // the same id twice and an id that is not there are the errors of an id
        assert_eq!(
            e.apply(add_fins(1, fins())).unwrap_err().code(),
            "PHOENIX-CAD-005"
        );
        assert_eq!(
            e.apply(Operation::RemoveFins { id: fid(9) })
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-005"
        );
    }

    #[test]
    fn a_set_that_does_not_fit_leaves_the_enclosure_as_it_was() {
        let mut e = enclosure();
        e.apply(Operation::AddHole {
            id: fid(5),
            hole: round_hole("vent", 30, 41),
        })
        .unwrap();
        let before = e.clone();
        // the hole at v 41 spans 38-44: 1 mm from the top fin (up to 37 mm), under the 2 mm web
        let err = e.apply(add_fins(1, fins())).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-043");
        assert!(
            err.to_string().contains("sink") && err.to_string().contains("vent"),
            "{err}"
        );
        assert_eq!(e, before);
        let mut thin = fins();
        thin.thickness = Length::new(700_000).unwrap();
        assert_eq!(
            e.apply(add_fins(1, thin)).unwrap_err().code(),
            "PHOENIX-CAD-043"
        );
        // a set on a face with no hole in the way goes in
        let mut front = fins();
        front.face = Face::Front;
        front.first = mm(10);
        front.length = mm(60);
        e.apply(add_fins(1, front)).unwrap();
    }

    #[test]
    fn a_hole_cannot_be_put_in_the_way_of_fins_and_a_hole_beside_them_can() {
        let mut e = enclosure();
        e.apply(add_fins(1, fins())).unwrap();
        let before = e.clone();
        // the fins cover u 10-60, v 19-37: a 6 mm hole at v 43 spans 40-46, 3 mm clear
        e.apply(Operation::AddHole {
            id: fid(5),
            hole: round_hole("beside", 30, 43),
        })
        .unwrap();
        let with_hole = e.clone();
        for (u, v) in [(50, 41), (30, 30), (63, 30)] {
            let err = e
                .apply(Operation::AddHole {
                    id: fid(6),
                    hole: round_hole("in the way", u, v),
                })
                .unwrap_err();
            assert_eq!(err.code(), "PHOENIX-CAD-043", "{u} {v}: {err}");
            assert_eq!(e, with_hole);
        }
        // moving the hole into the fins is refused too, and the hole stays where it was
        let err = e
            .apply(Operation::ReplaceHole {
                id: fid(5),
                hole: round_hole("beside", 30, 40),
            })
            .unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-043");
        assert_eq!(e, with_hole);
        assert_ne!(e, before);
        // a hole in another face does not care
        let mut front = round_hole("front", 30, 20);
        front.face = Face::Front;
        e.apply(Operation::AddHole {
            id: fid(7),
            hole: front,
        })
        .unwrap();
    }

    #[test]
    fn the_box_the_edges_and_the_lid_cannot_take_the_room_of_the_fins_away() {
        let mut e = enclosure();
        e.apply(add_fins(1, fins())).unwrap();
        let before = e.clone();
        // the fins end at u = 60 mm and the margin is 3 mm, so the top face has to be 63 mm wide:
        // an inner length of 59 mm (and two walls of 2 mm) is the least
        let mut narrow = *e.params();
        narrow.inner_length = mm(58);
        assert_eq!(
            e.apply(Operation::SetParams(narrow)).unwrap_err().code(),
            "PHOENIX-CAD-043"
        );
        assert_eq!(e, before);
        narrow.inner_length = mm(59);
        e.apply(Operation::SetParams(narrow)).unwrap();
        // rounded edges of 4 mm put the edge of the face 4 mm away (3 mm is the margin); the
        // fins start 3 mm from the face along v only when the first one is at 4 mm
        let mut near = fins();
        near.first = mm(4);
        let mut f = enclosure();
        f.apply(add_fins(2, near)).unwrap();
        let round = Edging {
            vertical: Treatment::Sharp,
            top: Treatment::Round(mm(4)),
            bottom: Treatment::Sharp,
        };
        let before = f.clone();
        assert_eq!(
            f.apply(Operation::SetEdging(round)).unwrap_err().code(),
            "PHOENIX-CAD-043"
        );
        assert_eq!(f, before);
        // the screws of the lid at the corners are holes of the top face too
        let lid = |inset: i64| Lid {
            fixing: LidFixing::Corners { inset: mm(inset) },
            screw: "M3".into(),
            hole_diameter: mm(3),
            style: HoleStyle::Plain,
            clearance: Length::ZERO,
            lip: None,
        };
        let mut g = enclosure();
        g.apply(add_fins(3, fins())).unwrap();
        let before = g.clone();
        // a screw 20 mm from the corner is at (22, 22), among the fins
        assert_eq!(
            g.apply(Operation::SetLid(Some(lid(20))))
                .unwrap_err()
                .code(),
            "PHOENIX-CAD-043"
        );
        assert_eq!(g, before);
        g.apply(Operation::SetLid(Some(lid(7)))).unwrap();
        assert!(g.lid().is_some());
    }

    #[test]
    fn two_sets_on_a_face_keep_the_air_between_them() {
        let mut e = enclosure();
        e.apply(add_fins(1, fins())).unwrap();
        let mut second = fins();
        second.name = "second".into();
        second.first = mm(40);
        let err = e.apply(add_fins(2, second.clone())).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-043");
        // the sets are checked in the order of their ids: the first one names the second
        assert!(
            err.to_string()
                .contains("sink: the fins are too near the set second"),
            "{err}"
        );
        second.first = mm(41);
        e.apply(add_fins(2, second)).unwrap();
        assert_eq!(e.fins().len(), 2);
    }

    // ---- the fins in the outputs ---------------------------------------------------------

    #[test]
    fn the_script_draws_each_fin_as_a_box_from_a_little_inside_the_wall() {
        let mut e = enclosure();
        e.apply(add_fins(1, fins())).unwrap();
        let text = crate::scad::to_scad(&e);
        // the top face of 104 x 64 mm: u is x and v is y; the lid is 3 mm thick, so the box
        // starts 1 mm inside it at z = 35 - 1 = 34 and rises to 35 + 10 = 45 (11 mm tall)
        assert!(text.contains("  // fins sink, top face\n"), "{text}");
        for y in [19, 27, 35] {
            assert!(
                text.contains(&format!("  translate([10, {y}, 34]) cube([50, 2, 11]);\n")),
                "{y}: {text}"
            );
        }
        assert!(text.contains("union() {\n"));
        // on the front face v is z and the fin rises towards -y: 1 mm inside the 2 mm wall
        let mut f = enclosure();
        let mut front = fins();
        front.face = Face::Front;
        front.first = mm(10);
        f.apply(add_fins(1, front)).unwrap();
        let text = crate::scad::to_scad(&f);
        assert!(
            text.contains("  translate([10, -10, 9]) cube([50, 11, 2]);\n"),
            "{text}"
        );
        // and the same model gives the same text
        assert_eq!(text, crate::scad::to_scad(&f.clone()));
    }

    #[test]
    fn the_step_file_refuses_fins_and_says_so() {
        let mut e = enclosure();
        e.apply(add_fins(1, fins())).unwrap();
        let err = crate::step::export_step(&e).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-022");
        assert!(err.to_string().contains("cooling fins"), "{err}");
        e.apply(Operation::RemoveFins { id: fid(1) }).unwrap();
        assert!(crate::step::export_step(&e).is_ok());
    }

    #[test]
    fn the_model_for_fabrication_keeps_the_fins_and_the_widened_holes_must_stay_clear_of_them() {
        let printer = Settings::defaults().printer;
        let mut e = enclosure();
        e.apply(add_fins(1, fins())).unwrap();
        // a 6 mm hole whose edge is 2.1 mm from the fins: 0.075 mm of compensation on each side
        // leaves 2.025 mm, still over the web of 2 mm
        e.apply(Operation::AddHole {
            id: fid(5),
            hole: Hole {
                name: "near".into(),
                v: Length::new(42_100_000).unwrap(),
                ..round_hole("near", 30, 0)
            },
        })
        .unwrap();
        let made = printer.for_fabrication(&e).unwrap();
        assert_eq!(made.fins(), e.fins());
        // a hole 2.05 mm from the fins is fine as designed and not once it is widened
        let mut tight = enclosure();
        tight.apply(add_fins(1, fins())).unwrap();
        tight
            .apply(Operation::AddHole {
                id: fid(5),
                hole: Hole {
                    name: "tight".into(),
                    v: Length::new(42_050_000).unwrap(),
                    ..round_hole("tight", 30, 0)
                },
            })
            .unwrap();
        let err = printer.for_fabrication(&tight).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-043");
    }

    #[test]
    fn the_views_draw_twelve_lines_for_each_fin_and_the_selected_set_in_its_colour() {
        use crate::scene::{scene_of, Highlight, Kind};
        let mut e = enclosure();
        e.apply(add_fins(1, fins())).unwrap();
        let plain = scene_of(&e, None, Highlight::Nothing, 8);
        assert_eq!(plain.iter().filter(|s| s.kind == Kind::Post).count(), 36);
        let picked = scene_of(&e, None, Highlight::Object(fid(1)), 8);
        assert_eq!(
            picked.iter().filter(|s| s.kind == Kind::Selected).count(),
            36
        );
        assert_eq!(picked.iter().filter(|s| s.kind == Kind::Post).count(), 0);
        // the lines rise 10 mm above the lid (z from 35 to 45)
        let top = picked
            .iter()
            .filter(|s| s.kind == Kind::Selected)
            .map(|s| s.a[2].max(s.b[2]))
            .fold(f64::MIN, f64::max);
        assert!((top - 45.0).abs() < 1e-9, "{top}");
    }

    #[test]
    fn the_volume_of_the_material_counts_the_fins_and_the_mass_says_it_leaves_them_out() {
        let bare = enclosure();
        let mut e = enclosure();
        e.apply(add_fins(1, fins())).unwrap();
        let a = bare.material_volume_with_holes().unwrap();
        let b = e.material_volume_with_holes().unwrap();
        assert_eq!(b.nm3 - a.nm3, 3_000i128 * 1_000_000_000_000_000_000);
        assert!(a.exact && b.exact);
        assert!(bare.mass_properties(1240).unwrap().exact);
        assert!(!e.mass_properties(1240).unwrap().exact);
    }
}
