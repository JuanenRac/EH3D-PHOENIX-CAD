//! Whether an envelope fits in an enclosure.
//!
//! The envelope is what the board tool tells the mechanical tool: the size of the outline of the
//! board and the height of its tallest part, plus the clearance wanted around it. The check is a
//! comparison of three sizes in integer nanometres; it does not look at screws, connectors or
//! holes.

use crate::enclosure::Enclosure;
use crate::Length;

/// The size of what has to fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Envelope {
    /// Size along X of the outline of the board.
    pub length: Length,
    /// Size along Y of the outline of the board.
    pub width: Length,
    /// The height from the underside of the board to the top of the tallest part.
    pub height: Length,
}

/// The answer, with the room left on each axis (negative when it does not fit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FitReport {
    /// True when it fits as placed.
    pub fits: bool,
    /// Inner size minus the envelope and twice the clearance, along X, in nanometres.
    pub spare_length: i64,
    /// The same along Y.
    pub spare_width: i64,
    /// The same along Z (the clearance counted once above and once below).
    pub spare_height: i64,
    /// True when it would fit turned a quarter of a turn about the vertical axis.
    pub fits_turned: bool,
}

/// Checks the envelope against the inner space with `clearance` all around.
pub fn check_fit(enclosure: &Enclosure, envelope: &Envelope, clearance: Length) -> FitReport {
    let p = enclosure.params();
    let c = 2 * clearance.get();
    let spare_length = p.inner_length.get() - envelope.length.get() - c;
    let spare_width = p.inner_width.get() - envelope.width.get() - c;
    let spare_height = p.inner_height.get() - envelope.height.get() - c;
    let turned_length = p.inner_length.get() - envelope.width.get() - c;
    let turned_width = p.inner_width.get() - envelope.length.get() - c;
    FitReport {
        fits: spare_length >= 0 && spare_width >= 0 && spare_height >= 0,
        spare_length,
        spare_width,
        spare_height,
        fits_turned: turned_length >= 0 && turned_width >= 0 && spare_height >= 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclosure::EnclosureParams;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

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

    fn envelope(l: i64, w: i64, h: i64) -> Envelope {
        Envelope {
            length: mm(l),
            width: mm(w),
            height: mm(h),
        }
    }

    #[test]
    fn a_board_that_fits_reports_its_spare_room() {
        let r = check_fit(&enclosure(), &envelope(90, 50, 20), mm(2));
        assert!(r.fits);
        assert_eq!(
            (r.spare_length, r.spare_width, r.spare_height),
            (6_000_000, 6_000_000, 6_000_000)
        );
    }

    #[test]
    fn exactly_filling_the_space_fits_and_one_nanometre_more_does_not() {
        let tight = envelope(96, 56, 26);
        assert!(check_fit(&enclosure(), &tight, mm(2)).fits);
        let over = Envelope {
            length: Length::new(96_000_001).unwrap(),
            ..tight
        };
        let r = check_fit(&enclosure(), &over, mm(2));
        assert!(!r.fits);
        assert_eq!(r.spare_length, -1);
    }

    #[test]
    fn a_board_that_fits_only_turned_is_told_so() {
        // 55 x 95 does not fit 100 x 60 as placed (95 > 60), it fits turned
        let r = check_fit(&enclosure(), &envelope(55, 95, 20), Length::ZERO);
        assert!(!r.fits);
        assert!(r.fits_turned);
        let r = check_fit(&enclosure(), &envelope(200, 200, 20), Length::ZERO);
        assert!(!r.fits && !r.fits_turned);
        // too tall: no turning helps
        let r = check_fit(&enclosure(), &envelope(10, 10, 31), Length::ZERO);
        assert!(!r.fits && !r.fits_turned);
    }
}
