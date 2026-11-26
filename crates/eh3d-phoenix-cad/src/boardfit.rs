//! The box against the board of the other branch: a box designed round its outline with posts
//! under its mounting holes, the update of the posts when the board moves, the check that every
//! part of the board has room, and the openings that its connectors need in the walls. Everything
//! is in whole nanometres on boxes and circles; the shape of a part is its rectangle and its
//! height, so a pass here is not a promise that the real parts fit.

use std::collections::BTreeSet;

use crate::boardart::{id_from, seat, BoardArtifact, Connector, Edge, Part, Seat};
use crate::body::{Step, StepState};
use crate::config::Settings;
use crate::enclosure::{Enclosure, EnclosureParams, Face, Hole, HoleStyle, Id, Operation};
use crate::mechanical::Mechanical;
use crate::mount::{posts_for_board, BoreKind, PostRequest};
use crate::parametric::ParamModel;
use crate::vars::Variables;
use crate::{CadError, Length};

/// The sizes a box made from a board starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxRequest<'a> {
    /// The room between the board and the walls.
    pub gap: Length,
    /// The room above the tallest part.
    pub headroom: Length,
    /// The height of the posts under the board.
    pub post_height: Length,
    /// The screw of the posts, from the table of the settings.
    pub screw: &'a str,
    /// An insert or a self-tapping screw.
    pub kind: BoreKind,
    /// The walls, the floor, the lid and the margins (the inner sizes are replaced).
    pub shell: EnclosureParams,
}

/// A recipe of a box round the board: the inner space holds the board with `gap` on every side
/// and `headroom` above its tallest part, and a post stands under every mounting hole.
pub fn box_for_board(
    settings: &Settings,
    artifact: &BoardArtifact,
    request: &BoxRequest<'_>,
) -> Result<Mechanical, CadError> {
    if request.gap.get() < 0 || request.headroom.get() < 0 || request.post_height.get() <= 0 {
        return Err(CadError::BoardFit(
            "the gap and the room above are not negative and the posts have a height".to_owned(),
        ));
    }
    let mut board = artifact.board.clone();
    let ((min_x, min_y), (max_x, max_y)) = board
        .bounds()
        .ok_or_else(|| CadError::BoardArtifact("the board has no outline".to_owned()))?;
    let tallest = board.tallest_component.map_or(0, Length::get);
    let gap = request.gap.get();
    let params = EnclosureParams {
        inner_length: Length::new(max_x - min_x + 2 * gap)?,
        inner_width: Length::new(max_y - min_y + 2 * gap)?,
        inner_height: Length::new(
            request.post_height.get() + board.thickness.get() + tallest + request.headroom.get(),
        )?,
        ..request.shell
    };
    board.placement = Some((request.gap, request.gap));
    let enclosure = Enclosure::new(params)?;
    let ops = posts_for_board(
        settings,
        &enclosure,
        &board,
        &PostRequest {
            screw: request.screw,
            kind: request.kind,
            height: request.post_height,
            base: false,
        },
    )?;
    let mut mech = Mechanical::new(ParamModel::new(params)?, Variables::new());
    mech.steps = ops
        .into_iter()
        .zip(board.holes.iter())
        .map(|(op, hole)| Step {
            name: format!("post {}", hole.name),
            op,
            suppressed: false,
        })
        .collect();
    mech.tip = mech.steps.len();
    mech.board = Some(board);
    Ok(mech)
}

/// What the update of the board of a recipe did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Update {
    /// Posts moved to follow their hole.
    pub moved: Vec<String>,
    /// Posts whose hole is gone; they stay where they were.
    pub orphans: Vec<String>,
    /// Holes without a post; none is added by itself.
    pub bare: Vec<String>,
    /// Steps that cannot be applied after the update.
    pub failed: Vec<String>,
}

/// Puts the board of the artifact in a recipe, keeping the placement it had, and moves every post
/// that was made for a mounting hole to where that hole is now. The steps keep their names and
/// their identities. What cannot be followed is told, never guessed.
pub fn update_board(
    settings: &Settings,
    mech: &mut Mechanical,
    artifact: &BoardArtifact,
) -> Result<Update, CadError> {
    let mut board = artifact.board.clone();
    board.placement = mech.board.as_ref().and_then(|b| b.placement);
    mech.board = Some(board.clone());
    let built = mech.build(&settings.constants, &settings.expr)?;
    let place = seat(&built.enclosure, &board)?;
    let mut out = Update::default();
    let mut followed: BTreeSet<Id> = BTreeSet::new();
    for step in &mut mech.steps {
        let Operation::AddPost { post, .. } = &mut step.op else {
            continue;
        };
        let Some(source) = post.board_hole else {
            continue;
        };
        match board.holes.iter().find(|h| h.id == source) {
            Some(hole) => {
                followed.insert(source);
                let x = Length::new(place.x + hole.x.get() - place.min_x)?;
                let y = Length::new(place.y + hole.y.get() - place.min_y)?;
                if (x, y) != (post.x, post.y) {
                    post.x = x;
                    post.y = y;
                    out.moved.push(step.name.clone());
                }
            }
            None => out.orphans.push(step.name.clone()),
        }
    }
    out.bare = board
        .holes
        .iter()
        .filter(|h| !followed.contains(&h.id))
        .map(|h| h.name.clone())
        .collect();
    let built = mech.build(&settings.constants, &settings.expr)?;
    out.failed = built
        .states
        .iter()
        .zip(&mech.steps)
        .filter(|(state, _)| matches!(state, StepState::Failed(_)))
        .map(|(_, step)| step.name.clone())
        .collect();
    Ok(out)
}

/// Why a part of the board has no room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trouble {
    /// It reaches beyond the inner space seen from above.
    Outside,
    /// A part of the top face reaches the lid.
    Lid,
    /// A part under the board reaches the floor.
    Floor,
    /// A part under the board meets a post.
    Post,
}

/// One part without room.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The part.
    pub part: String,
    /// Why.
    pub trouble: Trouble,
    /// How much is missing, in nanometres (0 for a post).
    pub short: i64,
}

fn rectangle(place: &Seat, part: &Part) -> (i64, i64, i64, i64) {
    let x = place.x + part.x.get() - place.min_x;
    let y = place.y + part.y.get() - place.min_y;
    (x, y, x + part.width.get(), y + part.depth.get())
}

/// The parts of the board that have no room with `clearance` round them.
pub fn check_parts(
    enclosure: &Enclosure,
    artifact: &BoardArtifact,
    clearance: Length,
) -> Result<Vec<Problem>, CadError> {
    let place = seat(enclosure, &artifact.board)?;
    let params = enclosure.params();
    let (length, width) = (params.inner_length.get(), params.inner_width.get());
    let top_of_board = place.rise + artifact.board.thickness.get();
    let c = clearance.get().max(0);
    let mut out = Vec::new();
    for part in &artifact.parts {
        let (x0, y0, x1, y1) = rectangle(&place, part);
        let mut tell = |trouble, short| {
            out.push(Problem {
                part: part.name.clone(),
                trouble,
                short,
            });
        };
        let beyond = (c - x0)
            .max(c - y0)
            .max(x1 + c - length)
            .max(y1 + c - width);
        if beyond > 0 {
            tell(Trouble::Outside, beyond);
        }
        if part.bottom {
            let short = part.height.get() + c - place.rise;
            if short > 0 {
                tell(Trouble::Floor, short);
            }
            let meets = enclosure.posts().values().any(|post| {
                // the nearest point of the rectangle to the centre of the post, twice the size
                // so that half a nanometre of a radius is not lost
                let px = post.x.get();
                let py = post.y.get();
                let dx = i128::from(px.clamp(x0, x1) - px) * 2;
                let dy = i128::from(py.clamp(y0, y1) - py) * 2;
                let reach = i128::from(post.outer_diameter.get()) + 2 * i128::from(c);
                dx * dx + dy * dy < reach * reach
            });
            if meets {
                tell(Trouble::Post, 0);
            }
        } else {
            let short = top_of_board + part.height.get() + c - params.inner_height.get();
            if short > 0 {
                tell(Trouble::Lid, short);
            }
        }
    }
    Ok(out)
}

fn face_of(edge: Edge) -> Face {
    match edge {
        Edge::Front => Face::Front,
        Edge::Back => Face::Back,
        Edge::Left => Face::Left,
        Edge::Right => Face::Right,
    }
}

/// The rectangular opening in a wall for one connector, with `clearance` on every side. The
/// position is measured on the face as it is seen from outside.
pub fn opening(
    enclosure: &Enclosure,
    artifact: &BoardArtifact,
    connector: &Connector,
    clearance: Length,
) -> Result<(Id, Hole), CadError> {
    let place = seat(enclosure, &artifact.board)?;
    let params = enclosure.params();
    let (outer_x, outer_y, _) = params.outer();
    let wall = params.wall.get();
    let at = connector.at.get();
    let u = match connector.edge {
        Edge::Front => wall + place.x + at - place.min_x,
        Edge::Back => outer_x.get() - (wall + place.x + at - place.min_x),
        Edge::Left => outer_y.get() - (wall + place.y + at - place.min_y),
        Edge::Right => wall + place.y + at - place.min_y,
    };
    let c = clearance.get().max(0);
    let v = params.floor.get()
        + place.rise
        + artifact.board.thickness.get()
        + connector.above.get()
        + connector.height.get() / 2;
    let id = id_from(&format!("connector {}", connector.name))?;
    Ok((
        id,
        Hole {
            name: format!("connector {}", connector.name),
            face: face_of(connector.edge),
            u: Length::new(u)?,
            v: Length::new(v)?,
            diameter: Length::new(connector.width.get() + 2 * c)?,
            style: HoleStyle::Rectangular {
                height: Length::new(connector.height.get() + 2 * c)?,
            },
        },
    ))
}

/// What became of the opening of one connector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A new step.
    Added,
    /// The step of this connector was changed.
    Moved,
    /// It was already as it should be.
    Same,
    /// The box refuses it, and why.
    Refused(CadError),
}

/// Adds or moves in a recipe the opening of every connector of the board. An opening the box
/// refuses (too near an edge, on another hole) is told and leaves the recipe without it.
pub fn apply_openings(
    settings: &Settings,
    mech: &mut Mechanical,
    artifact: &BoardArtifact,
    clearance: Length,
) -> Result<Vec<(String, Outcome)>, CadError> {
    let mut out = Vec::new();
    for connector in &artifact.connectors {
        let built = mech.build(&settings.constants, &settings.expr)?;
        let (id, hole) = opening(&built.enclosure, artifact, connector, clearance)?;
        let existing = mech
            .steps
            .iter()
            .position(|s| matches!(&s.op, Operation::AddHole { id: other, .. } if *other == id));
        let mut trial = built.enclosure.clone();
        let outcome = match built.enclosure.holes().get(&id) {
            Some(old) if *old == hole => Outcome::Same,
            Some(_) => match trial.apply(Operation::ReplaceHole {
                id,
                hole: hole.clone(),
            }) {
                Ok(_) => Outcome::Moved,
                Err(e) => Outcome::Refused(e),
            },
            None => match trial.apply(Operation::AddHole {
                id,
                hole: hole.clone(),
            }) {
                Ok(_) => Outcome::Added,
                Err(e) => Outcome::Refused(e),
            },
        };
        match (&outcome, existing) {
            (Outcome::Moved, Some(index)) => {
                if let Some(step) = mech.steps.get_mut(index) {
                    step.op = Operation::AddHole { id, hole };
                }
            }
            (Outcome::Added, _) => {
                // a new step goes at the tip, so that it is applied
                let tip = mech.tip.min(mech.steps.len());
                mech.steps.insert(
                    tip,
                    Step {
                        name: hole.name.clone(),
                        op: Operation::AddHole { id, hole },
                        suppressed: false,
                    },
                );
                mech.tip = tip + 1;
            }
            _ => {}
        }
        out.push((connector.name.clone(), outcome));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boardart::tests::{mm, sample};
    use crate::eh3dfile::tests::Seeded;
    use crate::uiconfig::UiSettings;

    fn request(shell: EnclosureParams) -> BoxRequest<'static> {
        BoxRequest {
            gap: mm(2),
            headroom: mm(3),
            post_height: mm(8),
            screw: "M3",
            kind: BoreKind::Insert,
            shell,
        }
    }

    fn made() -> (Settings, BoardArtifact, Mechanical) {
        let settings = Settings::defaults();
        let artifact = sample();
        let shell = UiSettings::defaults().new_box;
        let mech = box_for_board(&settings, &artifact, &request(shell)).unwrap();
        (settings, artifact, mech)
    }

    fn built(settings: &Settings, mech: &Mechanical) -> Enclosure {
        mech.build(&settings.constants, &settings.expr)
            .unwrap()
            .enclosure
    }

    #[test]
    fn the_box_round_a_board_has_the_worked_sizes_and_a_post_under_every_hole() {
        let (settings, artifact, mech) = made();
        let enclosure = built(&settings, &mech);
        let p = enclosure.params();
        assert_eq!(p.inner_length, mm(84));
        assert_eq!(p.inner_width, mm(54));
        // 8 of posts, 1.6 of board, 12 of the tallest part and 3 of room
        assert_eq!(p.inner_height.get(), 24_600_000);
        assert_eq!(enclosure.posts().len(), 4);
        assert!(enclosure
            .posts()
            .values()
            .any(|post| (post.x, post.y) == (mm(6), mm(6))));
        let place = seat(&enclosure, &artifact.board).unwrap();
        assert_eq!(place.rise, 8_000_000);
        assert!(check_parts(&enclosure, &artifact, Length::ZERO)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_part_without_room_is_named_with_what_is_missing() {
        let (settings, mut artifact, mech) = made();
        let enclosure = built(&settings, &mech);
        artifact.parts[0].height = mm(16);
        artifact.parts[1].height = mm(9);
        artifact.parts[1].x = mm(2);
        artifact.parts[1].y = mm(2);
        let found = check_parts(&enclosure, &artifact, Length::ZERO).unwrap();
        assert_eq!(
            found
                .iter()
                .map(|p| (p.part.as_str(), p.trouble, p.short))
                .collect::<Vec<_>>(),
            vec![
                ("C1", Trouble::Lid, 1_000_000),
                ("U2", Trouble::Floor, 1_000_000),
                ("U2", Trouble::Post, 0),
            ]
        );
        artifact.parts.truncate(1);
        artifact.parts[0].height = mm(1);
        artifact.parts[0].x = mm(75);
        let found = check_parts(&enclosure, &artifact, Length::ZERO).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].trouble, found[0].short),
            (Trouble::Outside, 3_000_000)
        );
    }

    #[test]
    fn the_posts_follow_the_holes_and_what_cannot_be_followed_is_told() {
        let (settings, artifact, mut mech) = made();
        let mut moved = artifact.clone();
        moved.board.holes[0].x = mm(10);
        moved.board.holes.remove(3);
        let ids: Vec<_> = mech.steps.iter().map(|s| s.name.clone()).collect();
        let update = update_board(&settings, &mut mech, &moved).unwrap();
        assert_eq!(update.moved, vec!["post H1".to_owned()]);
        assert_eq!(update.orphans, vec!["post H4".to_owned()]);
        assert!(
            update.bare.is_empty() && update.failed.is_empty(),
            "{update:?}"
        );
        assert_eq!(
            ids,
            mech.steps
                .iter()
                .map(|s| s.name.clone())
                .collect::<Vec<_>>()
        );
        let enclosure = built(&settings, &mech);
        assert!(enclosure
            .posts()
            .values()
            .any(|post| (post.x, post.y) == (mm(12), mm(6))));
        // the same board again moves nothing
        let again = update_board(&settings, &mut mech, &moved).unwrap();
        assert!(again.moved.is_empty());
    }

    #[test]
    fn the_openings_of_the_connectors_are_where_the_walls_are_seen_from_outside() {
        let (settings, mut artifact, mut mech) = made();
        // the board is 80 mm wide with 2 mm of gap: a connector at x = 40 is in the middle
        artifact.connectors[0].above = mm(1);
        let done = apply_openings(&settings, &mut mech, &artifact, mm(0)).unwrap();
        assert_eq!(done, vec![("J1".to_owned(), Outcome::Added)]);
        let enclosure = built(&settings, &mech);
        let hole = enclosure.holes().values().next().unwrap();
        let p = enclosure.params();
        assert_eq!(hole.face, Face::Front);
        assert_eq!(hole.u.get(), p.wall.get() + 42_000_000);
        assert_eq!(
            hole.v.get(),
            p.floor.get() + 8_000_000 + 1_600_000 + 1_000_000 + 2_500_000
        );
        let again = apply_openings(&settings, &mut mech, &artifact, mm(0)).unwrap();
        assert_eq!(again, vec![("J1".to_owned(), Outcome::Same)]);
        artifact.connectors[0].at = mm(30);
        let moved = apply_openings(&settings, &mut mech, &artifact, mm(0)).unwrap();
        assert_eq!(moved, vec![("J1".to_owned(), Outcome::Moved)]);
        // the same place seen from the back is measured from the other end
        artifact.connectors[0].edge = Edge::Back;
        let (_, back) = opening(&enclosure, &artifact, &artifact.connectors[0], mm(0)).unwrap();
        assert_eq!(back.u.get(), p.outer().0.get() - p.wall.get() - 32_000_000);
        artifact.connectors[0].width = mm(200);
        let refused = apply_openings(&settings, &mut mech, &artifact, mm(0)).unwrap();
        assert!(matches!(refused[0].1, Outcome::Refused(_)), "{refused:?}");
    }

    #[test]
    fn random_boards_make_boxes_whose_parts_are_checked_like_by_hand() {
        let settings = Settings::defaults();
        let shell = UiSettings::defaults().new_box;
        let mut random = Seeded(0x00CA_DB04);
        for _ in 0..40 {
            let mut artifact = sample();
            let height = 1 + random.below(30) as i64;
            artifact.parts.truncate(1);
            artifact.parts[0].height = mm(height);
            artifact.board.tallest_component = Some(mm(10));
            let mech = box_for_board(&settings, &artifact, &request(shell)).unwrap();
            let enclosure = built(&settings, &mech);
            let found = check_parts(&enclosure, &artifact, Length::ZERO).unwrap();
            // the box has 3 mm of room above a tallest part of 10 mm
            assert_eq!(found.is_empty(), height <= 13, "{height}");
        }
    }
}
