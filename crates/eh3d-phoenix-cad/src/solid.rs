//! The solid of the model as triangles, for the STL file.
//!
//! The box, its holes, its posts, the extrusions and pockets of the sketches, the cooling fins
//! and the centring lip of the lid are made as one closed surface, and every part placed from the
//! catalog is a closed surface of its own, in the place its mates give it. Nothing here is a
//! drawing: the pieces are the same shapes the model already knows (a hole is the cylinder, the
//! cone or the box that its style says, a post is a pillar with a foot and a bore), meshed with
//! circles of the same area as the true circle, so that the volume of the triangles is the volume
//! of the model up to the sides of the polygons of the sketches.
//!
//! The surface is made of flat regions and walls, never by cutting one solid out of another, so
//! the edges of two pieces meet at the same vertices and the surface is closed. That asks for each
//! shape to lie where the material is: a hole inside the thin part of its face, a post on the
//! floor away from the walls, a feature inside the flat part of its face, shapes that do not cross.
//! A model that breaks that is refused with `PHOENIX-CAD-048` and says which shape and which other
//! shape it crosses; so is a box whose edges around the lid or the floor are rounded or chamfered.
//! Every body is checked to be closed and to have the exact volume of the model within
//! `mesh.volume_tolerance_ppm` before it is given out (`PHOENIX-CAD-049`).

use crate::catalog::{Head, PartInstance, Shape};
use crate::config::MeshSettings;
use crate::enclosure::{Enclosure, Face, Feature, FeatureKind, Hole, HoleStyle, Treatment};
use crate::fins::FinSet;
use crate::mass::hole_volume_nm3;
use crate::mesh::{Builder, Circle, Loop, Mesh, Role, P3};
use crate::mount::{Lid, Lip, Post};
use crate::CadError;

/// One solid of the model and the exact volume it must have.
#[derive(Debug, Clone)]
pub struct Body {
    /// `enclosure`, or the name of the part.
    pub name: String,
    /// The surface.
    pub mesh: Mesh,
    /// The exact volume of the model in cubic nanometres.
    pub expected_nm3: f64,
}

fn shape_error(why: String) -> CadError {
    CadError::MeshShape(why)
}

fn index(face: Face) -> usize {
    match face {
        Face::Front => 0,
        Face::Back => 1,
        Face::Left => 2,
        Face::Right => 3,
        Face::Top => 4,
        Face::Bottom => 5,
    }
}

/// The normal of a face, out of the box.
fn out_normal(face: Face) -> P3 {
    match face {
        Face::Front => [0.0, -1.0, 0.0],
        Face::Back => [0.0, 1.0, 0.0],
        Face::Left => [-1.0, 0.0, 0.0],
        Face::Right => [1.0, 0.0, 0.0],
        Face::Top => [0.0, 0.0, 1.0],
        Face::Bottom => [0.0, 0.0, -1.0],
    }
}

fn negated(n: P3) -> P3 {
    [-n[0], -n[1], -n[2]]
}

fn nm(v: i64) -> f64 {
    v as f64
}

/// The sizes of the box in nanometres.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Frame {
    x: f64,
    y: f64,
    z: f64,
    wall: f64,
    floor: f64,
    lid: f64,
    inner_h: f64,
    vertical: f64,
}

impl Frame {
    pub(crate) fn of(enclosure: &Enclosure) -> Frame {
        let p = enclosure.params();
        let (x, y, z) = p.outer();
        Frame {
            x: nm(x.get()),
            y: nm(y.get()),
            z: nm(z.get()),
            wall: nm(p.wall.get()),
            floor: nm(p.floor.get()),
            lid: nm(p.lid.get()),
            inner_h: nm(p.inner_height.get()),
            vertical: nm(enclosure.edging().vertical.size().get()),
        }
    }

    /// The thickness of the material of a face where it is thin.
    fn thickness(&self, face: Face) -> f64 {
        match face {
            Face::Front | Face::Back | Face::Left | Face::Right => self.wall,
            Face::Top => self.lid,
            Face::Bottom => self.floor,
        }
    }

    /// The width (along `u`) and the height (along `v`) of a face.
    fn size(&self, face: Face) -> (f64, f64) {
        match face {
            Face::Front | Face::Back => (self.x, self.z),
            Face::Left | Face::Right => (self.y, self.z),
            Face::Top | Face::Bottom => (self.x, self.y),
        }
    }

    /// The point of a face at `u`, `v`, a depth `d` into the material (negative is outwards), the
    /// faces being seen from outside with `u` to the right and `v` up.
    pub(crate) fn xyz(&self, face: Face, u: f64, v: f64, d: f64) -> P3 {
        match face {
            Face::Front => [u, d, v],
            Face::Back => [self.x - u, self.y - d, v],
            Face::Left => [d, self.y - u, v],
            Face::Right => [self.x - d, u, v],
            Face::Top => [u, v, self.z - d],
            Face::Bottom => [u, self.y - v, d],
        }
    }

    /// The rectangle of the inner space as seen from a face: `u` from, `u` to, `v` from, `v` to.
    fn cavity(&self, face: Face) -> (f64, f64, f64, f64) {
        let (w, h) = self.size(face);
        match face {
            Face::Front | Face::Back | Face::Left | Face::Right => (
                self.wall,
                w - self.wall,
                self.floor,
                self.floor + self.inner_h,
            ),
            Face::Top | Face::Bottom => (self.wall, w - self.wall, self.wall, h - self.wall),
        }
    }
}

type Ring2 = (f64, Vec<(f64, f64)>);

fn rect2(u0: f64, v0: f64, u1: f64, v1: f64) -> Vec<(f64, f64)> {
    vec![(u0, v0), (u1, v0), (u1, v1), (u0, v1)]
}

struct Solid<'a> {
    b: Builder,
    circle: &'a Circle,
    f: Frame,
    round: bool,
    outer: [Vec<Loop>; 6],
    cavity: [Vec<Loop>; 6],
}

impl Solid<'_> {
    fn ring3(&mut self, face: Face, depth: f64, pts: &[(f64, f64)]) -> Vec<u32> {
        let places: Vec<P3> = pts
            .iter()
            .map(|p| self.f.xyz(face, p.0, p.1, depth))
            .collect();
        self.b.ring(&places)
    }

    /// The outline of the box seen from above, counter-clockwise: the corners rounded or chamfered
    /// by the treatment of the vertical edges. Each point says whether a flat side follows it.
    fn footprint(&self) -> Vec<(f64, f64, bool)> {
        let (x, y, s) = (self.f.x, self.f.y, self.f.vertical);
        let q = self.circle.sides() / 4;
        // the corners in the order the outline goes: the centre of the corner circle and the side
        // where its arc starts
        let corners = [
            (x - s, s, 3 * q),
            (x - s, y - s, 0),
            (s, y - s, q),
            (s, s, 2 * q),
        ];
        let mut out: Vec<(f64, f64, bool)> = Vec::new();
        for (cx, cy, start) in corners {
            let list: Vec<(f64, f64)> = if s == 0.0 {
                vec![(cx, cy)]
            } else {
                let arc = self.circle.arc(cx, cy, s, start, start + q);
                if self.round {
                    arc
                } else {
                    vec![arc[0], arc[arc.len() - 1]]
                }
            };
            let last = list.len() - 1;
            for (i, p) in list.iter().enumerate() {
                out.push((p.0, p.1, i == last));
            }
        }
        // a flat side that has gone to nothing leaves two equal points
        let mut cleaned: Vec<(f64, f64, bool)> = Vec::new();
        for p in out {
            match cleaned.last_mut() {
                Some(l) if l.0 == p.0 && l.1 == p.1 => l.2 = p.2,
                _ => cleaned.push(p),
            }
        }
        if cleaned.len() > 1 {
            let (first, last) = (cleaned[0], cleaned[cleaned.len() - 1]);
            if first.0 == last.0 && first.1 == last.1 {
                cleaned.pop();
            }
        }
        cleaned
    }

    /// The outside of the box, the inner space and their edges.
    fn shell(&mut self) -> Result<(), CadError> {
        let (z, s) = (self.f.z, self.f.vertical);
        let foot = self.footprint();
        let bottom_pts: Vec<P3> = foot.iter().map(|p| [p.0, p.1, 0.0]).collect();
        let top_pts: Vec<P3> = foot.iter().map(|p| [p.0, p.1, z]).collect();
        let (bottom, top) = (self.b.ring(&bottom_pts), self.b.ring(&top_pts));
        // the walls round the corners; the flat sides are regions of their own
        for i in 0..foot.len() {
            if foot[i].2 {
                continue;
            }
            let j = (i + 1) % foot.len();
            self.b.triangle(bottom[i], bottom[j], top[j])?;
            self.b.triangle(bottom[i], top[j], top[i])?;
        }
        self.outer[index(Face::Bottom)].push(Loop::new(bottom, Role::Boundary, "the floor"));
        self.outer[index(Face::Top)].push(Loop::new(top, Role::Boundary, "the lid"));
        for face in [Face::Front, Face::Back, Face::Left, Face::Right] {
            let (w, h) = self.f.size(face);
            if w - 2.0 * s > 0.0 {
                let pts = rect2(s, 0.0, w - s, h);
                let ids = self.ring3(face, 0.0, &pts);
                self.outer[index(face)].push(Loop::new(
                    ids,
                    Role::Boundary,
                    &format!("the {} face", face.name()),
                ));
            }
        }
        for face in Face::ALL {
            let (u0, u1, v0, v1) = self.f.cavity(face);
            let ids = self.ring3(face, self.f.thickness(face), &rect2(u0, v0, u1, v1));
            self.cavity[index(face)].push(Loop::new(
                ids,
                Role::Boundary,
                &format!("the inner space at the {} face", face.name()),
            ));
        }
        Ok(())
    }

    /// The rings of a hole, from its mouth to the inner side: the depth of each and its points;
    /// and for every pair of rings whether a flat step (the floor of a recess) lies between them
    /// instead of a wall.
    fn hole_rings(&self, h: &Hole, thickness: f64) -> Result<(Vec<Ring2>, Vec<bool>), CadError> {
        let (u, v) = (nm(h.u.get()), nm(h.v.get()));
        let d = nm(h.diameter.get());
        let round = |diameter: f64| self.circle.polygon(u, v, diameter / 2.0);
        let bad = |why: &str| shape_error(format!("hole {}: {why}", h.name));
        match h.style {
            HoleStyle::Plain => Ok((vec![(0.0, round(d)), (thickness, round(d))], vec![false])),
            HoleStyle::Countersunk { head_diameter } => {
                let head = nm(head_diameter.get());
                let deep = (head - d) / 2.0;
                if deep > thickness {
                    return Err(bad("the countersink is deeper than the material"));
                }
                if deep == thickness {
                    return Ok((vec![(0.0, round(head)), (thickness, round(d))], vec![false]));
                }
                Ok((
                    vec![(0.0, round(head)), (deep, round(d)), (thickness, round(d))],
                    vec![false, false],
                ))
            }
            HoleStyle::Counterbored {
                head_diameter,
                depth,
            } => {
                let (head, k) = (nm(head_diameter.get()), nm(depth.get()));
                if k >= thickness {
                    return Err(bad("the recess is as deep as the material"));
                }
                Ok((
                    vec![
                        (0.0, round(head)),
                        (k, round(head)),
                        (k, round(d)),
                        (thickness, round(d)),
                    ],
                    vec![false, true, false],
                ))
            }
            HoleStyle::Rectangular { height } => {
                let (w, t) = (d / 2.0, nm(height.get()) / 2.0);
                let pts = rect2(u - w, v - t, u + w, v + t);
                Ok((vec![(0.0, pts.clone()), (thickness, pts)], vec![false]))
            }
            HoleStyle::Oval { length, vertical } => {
                let sep = (nm(length.get()) - d) / 2.0;
                if sep <= 0.0 {
                    return Ok((vec![(0.0, round(d)), (thickness, round(d))], vec![false]));
                }
                let n = self.circle.sides();
                let r = self.circle.radius(d / 2.0);
                let mut pts = Vec::new();
                if vertical {
                    pts.extend(self.circle.arc(u, v - sep, r, n / 2, n));
                    pts.extend(self.circle.arc(u, v + sep, r, 0, n / 2));
                } else {
                    pts.extend(self.circle.arc(u + sep, v, r, 3 * n / 4, 3 * n / 4 + n / 2));
                    pts.extend(self.circle.arc(u - sep, v, r, n / 4, n / 4 + n / 2));
                }
                Ok((vec![(0.0, pts.clone()), (thickness, pts)], vec![false]))
            }
        }
    }

    /// A hole through the thin part of a face: the mouth in the outer face, the wall with its
    /// steps and the opening in the inner side.
    fn hole(&mut self, h: &Hole) -> Result<(), CadError> {
        let t = self.f.thickness(h.face);
        let (rings, flats) = self.hole_rings(h, t)?;
        let ids: Vec<Vec<u32>> = rings
            .iter()
            .map(|(depth, pts)| self.ring3(h.face, *depth, pts))
            .collect();
        let label = format!("the hole {}", h.name);
        let fi = index(h.face);
        self.outer[fi].push(Loop::new(ids[0].clone(), Role::Cut, &label));
        self.cavity[fi].push(Loop::new(ids[ids.len() - 1].clone(), Role::Cut, &label));
        for (i, flat) in flats.iter().enumerate() {
            if *flat {
                self.b.plane(
                    &[
                        Loop::new(ids[i].clone(), Role::Boundary, &label),
                        Loop::new(ids[i + 1].clone(), Role::Cut, &label),
                    ],
                    out_normal(h.face),
                )?;
            } else {
                self.b.strip(&ids[i], &ids[i + 1], false)?;
            }
        }
        Ok(())
    }

    /// A prism of outlines raised from a face: each outline with the outlines of its holes, a
    /// height outwards.
    fn raised(
        &mut self,
        face: Face,
        label: &str,
        outline: &[(f64, f64)],
        holes: &[Vec<(f64, f64)>],
        height: f64,
    ) -> Result<(), CadError> {
        let fi = index(face);
        let base = self.ring3(face, 0.0, outline);
        let top = self.ring3(face, -height, outline);
        self.outer[fi].push(Loop::new(base.clone(), Role::Cut, label));
        self.b.strip(&base, &top, true)?;
        let mut cap = vec![Loop::new(top, Role::Boundary, label)];
        for hole in holes {
            let hb = self.ring3(face, 0.0, hole);
            let ht = self.ring3(face, -height, hole);
            self.outer[fi].push(Loop::new(hb.clone(), Role::Island, label));
            self.b.strip(&hb, &ht, false)?;
            cap.push(Loop::new(ht, Role::Cut, label));
        }
        self.b.plane(&cap, out_normal(face))
    }

    /// A pocket cut into a face: the outline with the outlines of its islands, a depth inwards.
    fn pocket(
        &mut self,
        face: Face,
        label: &str,
        outline: &[(f64, f64)],
        islands: &[Vec<(f64, f64)>],
        depth: f64,
    ) -> Result<(), CadError> {
        let fi = index(face);
        let thickness = self.f.thickness(face);
        let through = depth >= thickness;
        let bottom_plane = if through { thickness } else { depth };
        let mouth = self.ring3(face, 0.0, outline);
        let floor = self.ring3(face, bottom_plane, outline);
        self.outer[fi].push(Loop::new(mouth.clone(), Role::Cut, label));
        self.b.strip(&mouth, &floor, false)?;
        let mut flat = vec![Loop::new(floor.clone(), Role::Boundary, label)];
        if through {
            self.cavity[fi].push(Loop::new(floor, Role::Cut, label));
            flat.clear();
        }
        for island in islands {
            let top = self.ring3(face, 0.0, island);
            let low = self.ring3(face, bottom_plane, island);
            self.outer[fi].push(Loop::new(top.clone(), Role::Island, label));
            self.b.strip(&top, &low, true)?;
            if through {
                self.cavity[fi].push(Loop::new(low, Role::Island, label));
            } else {
                flat.push(Loop::new(low, Role::Cut, label));
            }
        }
        if !flat.is_empty() {
            self.b.plane(&flat, out_normal(face))?;
        }
        Ok(())
    }

    /// An extrusion or a pocket of a sketch.
    fn feature(&mut self, f: &Feature, thickness: f64) -> Result<(), CadError> {
        let label = format!("the sketch {}", f.sketch);
        let (u0, v0) = (nm(f.u0.get()), nm(f.v0.get()));
        let shift = |pts: &[(i64, i64)]| -> Vec<(f64, f64)> {
            pts.iter().map(|p| (u0 + nm(p.0), v0 + nm(p.1))).collect()
        };
        let depth = f.depth.map_or(thickness, |d| nm(d.get()));
        for region in &f.profile.regions {
            let outline = shift(region.outer.polygon());
            let holes: Vec<Vec<(f64, f64)>> =
                region.holes.iter().map(|h| shift(h.polygon())).collect();
            match f.kind {
                FeatureKind::Extrude => self.raised(f.face, &label, &outline, &holes, depth)?,
                FeatureKind::Pocket => self.pocket(f.face, &label, &outline, &holes, depth)?,
            }
        }
        Ok(())
    }

    /// The fins of a set, one box each.
    fn fins(&mut self, set: &FinSet) -> Result<(), CadError> {
        let label = format!("the fins {}", set.name);
        let height = nm(set.height.get());
        for r in set.rects() {
            let half = |v: i128| v as f64 / 2.0;
            let rect = rect2(half(r.u0), half(r.v0), half(r.u1), half(r.v1));
            self.raised(set.face, &label, &rect, &[], height)?;
        }
        Ok(())
    }

    /// A post on the floor: the pillar with its foot, the top ring and the bore.
    fn post(&mut self, p: &Post) -> Result<(), CadError> {
        let label = format!("the post {}", p.name);
        let (cx, cy) = (self.f.wall + nm(p.x.get()), self.f.wall + nm(p.y.get()));
        let z0 = self.f.floor;
        let (d, h) = (nm(p.outer_diameter.get()), nm(p.height.get()));
        let (bore, depth) = (nm(p.bore_diameter.get()), nm(p.bore_depth.get()));
        let at = |this: &mut Self, diameter: f64, z: f64| -> Vec<u32> {
            let pts: Vec<P3> = this
                .circle
                .polygon(cx, cy, diameter / 2.0)
                .iter()
                .map(|q| [q.0, q.1, z])
                .collect();
            this.b.ring(&pts)
        };
        let (bottom, upper) = match p.base {
            Some(base) => {
                let (bd, bh) = (nm(base.diameter.get()), nm(base.height.get()));
                let wide = at(self, bd, z0);
                let narrow = at(self, d, z0 + bh);
                self.b.strip(&wide, &narrow, true)?;
                (wide, narrow)
            }
            None => {
                let ring = at(self, d, z0);
                (ring.clone(), ring)
            }
        };
        let top = at(self, d, z0 + h);
        self.b.strip(&upper, &top, true)?;
        self.cavity[index(Face::Bottom)].push(Loop::new(bottom, Role::Cut, &label));
        let mouth = at(self, bore, z0 + h);
        let end = at(self, bore, z0 + h - depth);
        self.b.plane(
            &[
                Loop::new(top, Role::Boundary, &label),
                Loop::new(mouth.clone(), Role::Cut, &label),
            ],
            [0.0, 0.0, 1.0],
        )?;
        self.b.strip(&mouth, &end, false)?;
        if depth >= h {
            self.cavity[index(Face::Bottom)].push(Loop::new(end, Role::Island, &label));
        } else {
            self.b
                .plane(&[Loop::new(end, Role::Boundary, &label)], [0.0, 0.0, 1.0])?;
        }
        Ok(())
    }

    /// The centring lip under the lid: a rectangular ring.
    fn lip(&mut self, lid: &Lid, lip: &Lip) -> Result<(), CadError> {
        let label = "the lip of the lid";
        let (c, t, depth) = (
            nm(lid.clearance.get()),
            nm(lip.thickness.get()),
            nm(lip.depth.get()),
        );
        let w = self.f.wall;
        let (x1, y1) = (self.f.x - w - c, self.f.y - w - c);
        let (o0, i0) = (w + c, w + c + t);
        let ceiling = self.f.z - self.f.lid;
        let at = |this: &mut Self, lo: (f64, f64), hi: (f64, f64), z: f64| -> Vec<u32> {
            this.b.ring(&[
                [lo.0, lo.1, z],
                [hi.0, lo.1, z],
                [hi.0, hi.1, z],
                [lo.0, hi.1, z],
            ])
        };
        let outer_hi = (x1, y1);
        let inner_hi = (x1 - t, y1 - t);
        let outer_up = at(self, (o0, o0), outer_hi, ceiling);
        let outer_down = at(self, (o0, o0), outer_hi, ceiling - depth);
        let inner_up = at(self, (i0, i0), inner_hi, ceiling);
        let inner_down = at(self, (i0, i0), inner_hi, ceiling - depth);
        let top = index(Face::Top);
        self.cavity[top].push(Loop::new(outer_up.clone(), Role::Cut, label));
        self.cavity[top].push(Loop::new(inner_up.clone(), Role::Island, label));
        self.b.strip(&outer_up, &outer_down, true)?;
        self.b.strip(&inner_up, &inner_down, false)?;
        self.b.plane(
            &[
                Loop::new(outer_down, Role::Boundary, label),
                Loop::new(inner_down, Role::Cut, label),
            ],
            [0.0, 0.0, -1.0],
        )
    }

    /// Cuts every region of the faces into triangles.
    fn emit(&mut self) -> Result<(), CadError> {
        for face in Face::ALL {
            let fi = index(face);
            let outer = std::mem::take(&mut self.outer[fi]);
            self.b.plane(&outer, out_normal(face))?;
            let cavity = std::mem::take(&mut self.cavity[fi]);
            self.b.plane(&cavity, negated(out_normal(face)))?;
        }
        Ok(())
    }
}

/// Whether a model has a shape that the solid does not make.
fn check_edges(enclosure: &Enclosure) -> Result<(), CadError> {
    let e = enclosure.edging();
    if !e.top.is_sharp() || !e.bottom.is_sharp() {
        return Err(shape_error(
            "the edges around the lid or the floor are rounded or chamfered".to_owned(),
        ));
    }
    Ok(())
}

/// The surface of the box with everything on it, as one closed mesh.
pub fn enclosure_mesh(
    enclosure: &Enclosure,
    circle: &Circle,
    limit: usize,
) -> Result<Mesh, CadError> {
    check_edges(enclosure)?;
    let mut s = Solid {
        b: Builder::new(limit),
        circle,
        f: Frame::of(enclosure),
        round: matches!(enclosure.edging().vertical, Treatment::Round(_)),
        outer: Default::default(),
        cavity: Default::default(),
    };
    s.shell()?;
    for hole in enclosure.all_holes() {
        s.hole(&hole)?;
    }
    for feature in enclosure.features().values() {
        let thickness = s.f.thickness(feature.face);
        s.feature(feature, thickness)?;
    }
    for set in enclosure.fins().values() {
        s.fins(set)?;
    }
    for post in enclosure.posts().values() {
        s.post(post)?;
    }
    if let Some(lid) = enclosure.lid() {
        if let Some(lip) = &lid.lip {
            s.lip(lid, lip)?;
        }
    }
    s.emit()?;
    Ok(s.b.finish())
}

/// The exact volume of the box with everything on it, in cubic nanometres: the outer box less the
/// treatment of the vertical edges and the inner space and the holes (the lid fixing among them),
/// plus the posts, the lip, the extrusions and the fins, less the pockets.
pub fn enclosure_volume_nm3(enclosure: &Enclosure) -> Result<f64, CadError> {
    let pi = std::f64::consts::PI;
    let p = enclosure.params();
    let mut v = enclosure.outer_volume_nm3() as f64
        - enclosure.edging_volume()?.nm3 as f64
        - nm(p.inner_length.get()) * nm(p.inner_width.get()) * nm(p.inner_height.get());
    for hole in enclosure.all_holes() {
        let t = match hole.face {
            Face::Front | Face::Back | Face::Left | Face::Right => p.wall,
            Face::Top => p.lid,
            Face::Bottom => p.floor,
        };
        v -= hole_volume_nm3(hole.diameter, hole.style, t)? as f64;
    }
    for post in enclosure.posts().values() {
        let (d, h) = (nm(post.outer_diameter.get()), nm(post.height.get()));
        v += pi / 4.0 * d * d * h;
        if let Some(base) = post.base {
            let (bd, bh) = (nm(base.diameter.get()), nm(base.height.get()));
            v += pi / 12.0 * bh * (bd * bd + bd * d + d * d) - pi / 4.0 * d * d * bh;
        }
        let (b, depth) = (nm(post.bore_diameter.get()), nm(post.bore_depth.get()));
        v -= pi / 4.0 * b * b * depth;
    }
    if let Some(lid) = enclosure.lid() {
        if let Some(lip) = &lid.lip {
            let (c, t, depth) = (
                nm(lid.clearance.get()),
                nm(lip.thickness.get()),
                nm(lip.depth.get()),
            );
            let (l, w) = (
                nm(p.inner_length.get()) - 2.0 * c,
                nm(p.inner_width.get()) - 2.0 * c,
            );
            v += (l * w - (l - 2.0 * t) * (w - 2.0 * t)) * depth;
        }
    }
    for feature in enclosure.features().values() {
        let depth = nm(feature.depth_in(p).get());
        let volume = feature.profile.area_nm2() * depth;
        match feature.kind {
            FeatureKind::Extrude => v += volume,
            FeatureKind::Pocket => v -= volume,
        }
    }
    for set in enclosure.fins().values() {
        v += set.volume_nm3() as f64;
    }
    Ok(v)
}

// ---------------------------------------------------------------------------------------------
// The parts of the catalog
// ---------------------------------------------------------------------------------------------

fn rect_ring(b: &mut Builder, x0: f64, y0: f64, x1: f64, y1: f64, z: f64) -> Vec<u32> {
    b.ring(&[[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]])
}

fn circle_ring(b: &mut Builder, circle: &Circle, c: (f64, f64), r: f64, z: f64) -> Vec<u32> {
    let pts: Vec<P3> = circle
        .polygon(c.0, c.1, r)
        .iter()
        .map(|p| [p.0, p.1, z])
        .collect();
    b.ring(&pts)
}

/// A bar of a rectangular section, solid or hollow, along `z`.
fn profile_mesh(size: [f64; 3], wall: f64, limit: usize) -> Result<Mesh, CadError> {
    let mut b = Builder::new(limit);
    let [w, h, len] = size;
    let (lo, hi) = (
        rect_ring(&mut b, 0.0, 0.0, w, h, 0.0),
        rect_ring(&mut b, 0.0, 0.0, w, h, len),
    );
    b.strip(&lo, &hi, true)?;
    let mut bottom = vec![Loop::new(lo, Role::Boundary, "the bar")];
    let mut top = vec![Loop::new(hi, Role::Boundary, "the bar")];
    if wall > 0.0 {
        let (il, ih) = (
            rect_ring(&mut b, wall, wall, w - wall, h - wall, 0.0),
            rect_ring(&mut b, wall, wall, w - wall, h - wall, len),
        );
        b.strip(&il, &ih, false)?;
        bottom.push(Loop::new(il, Role::Cut, "the hollow of the bar"));
        top.push(Loop::new(ih, Role::Cut, "the hollow of the bar"));
    }
    b.plane(&bottom, [0.0, 0.0, -1.0])?;
    b.plane(&top, [0.0, 0.0, 1.0])?;
    Ok(b.finish())
}

/// A screw with its head at the smallest `z` and its shank running up.
fn screw_mesh(
    circle: &Circle,
    shank: f64,
    length: f64,
    head: Head,
    head_diameter: f64,
    head_height: f64,
    limit: usize,
) -> Result<Mesh, CadError> {
    let mut b = Builder::new(limit);
    let wide = shank.max(head_diameter);
    let c = (wide / 2.0, wide / 2.0);
    let label = "the screw";
    let s0 = circle_ring(&mut b, circle, c, shank / 2.0, head_height);
    let s1 = circle_ring(&mut b, circle, c, shank / 2.0, head_height + length);
    b.strip(&s0, &s1, true)?;
    b.plane(&[Loop::new(s1, Role::Boundary, label)], [0.0, 0.0, 1.0])?;
    let h0: Vec<u32> = match head {
        Head::Hex => {
            let r = head_diameter / 3.0_f64.sqrt();
            let half = 3.0_f64.sqrt() / 2.0;
            let unit = [
                (1.0, 0.0),
                (0.5, half),
                (-0.5, half),
                (-1.0, 0.0),
                (-0.5, -half),
                (0.5, -half),
            ];
            let at = |b: &mut Builder, z: f64| -> Vec<u32> {
                let pts: Vec<P3> = unit
                    .iter()
                    .map(|u| [c.0 + r * u.0, c.1 + r * u.1, z])
                    .collect();
                b.ring(&pts)
            };
            let (lo, hi) = (at(&mut b, 0.0), at(&mut b, head_height));
            b.strip(&lo, &hi, true)?;
            b.plane(
                &[
                    Loop::new(hi, Role::Boundary, label),
                    Loop::new(s0, Role::Cut, label),
                ],
                [0.0, 0.0, 1.0],
            )?;
            lo
        }
        Head::Cylinder => {
            let lo = circle_ring(&mut b, circle, c, head_diameter / 2.0, 0.0);
            let hi = circle_ring(&mut b, circle, c, head_diameter / 2.0, head_height);
            b.strip(&lo, &hi, true)?;
            b.plane(
                &[
                    Loop::new(hi, Role::Boundary, label),
                    Loop::new(s0, Role::Cut, label),
                ],
                [0.0, 0.0, 1.0],
            )?;
            lo
        }
        Head::Countersunk => {
            let lo = circle_ring(&mut b, circle, c, head_diameter / 2.0, 0.0);
            b.strip(&lo, &s0, true)?;
            lo
        }
    };
    b.plane(&[Loop::new(h0, Role::Boundary, label)], [0.0, 0.0, -1.0])?;
    Ok(b.finish())
}

/// An angle of two legs along `x` and `z`, `width` across `y`, with a hole in each leg.
fn bracket_mesh(
    circle: &Circle,
    legs: (f64, f64),
    width: f64,
    t: f64,
    hole: f64,
    limit: usize,
) -> Result<Mesh, CadError> {
    let mut b = Builder::new(limit);
    let (a, c) = legs;
    let w = width;
    let label = "the bracket";
    let hole_label = "the hole of the bracket";
    // the face under leg a, its end, its top, the inner face of leg b, its end and its outside
    let q = |b: &mut Builder, pts: [[f64; 3]; 4]| b.ring(&pts);
    let mut floor = vec![Loop::new(
        q(
            &mut b,
            [[0.0, 0.0, 0.0], [a, 0.0, 0.0], [a, w, 0.0], [0.0, w, 0.0]],
        ),
        Role::Boundary,
        label,
    )];
    let mut shelf = vec![Loop::new(
        q(&mut b, [[t, 0.0, t], [a, 0.0, t], [a, w, t], [t, w, t]]),
        Role::Boundary,
        label,
    )];
    let mut inner = vec![Loop::new(
        q(&mut b, [[t, 0.0, t], [t, w, t], [t, w, c], [t, 0.0, c]]),
        Role::Boundary,
        label,
    )];
    let mut back = vec![Loop::new(
        q(
            &mut b,
            [[0.0, 0.0, 0.0], [0.0, w, 0.0], [0.0, w, c], [0.0, 0.0, c]],
        ),
        Role::Boundary,
        label,
    )];
    let end_a = q(&mut b, [[a, 0.0, 0.0], [a, w, 0.0], [a, w, t], [a, 0.0, t]]);
    let end_c = q(&mut b, [[0.0, 0.0, c], [t, 0.0, c], [t, w, c], [0.0, w, c]]);
    if hole > 0.0 {
        let ax = t + (a - t) / 2.0;
        let az = t + (c - t) / 2.0;
        let ring = |b: &mut Builder, along_z: bool, at: f64| -> Vec<u32> {
            let pts: Vec<P3> = circle
                .polygon(0.0, 0.0, hole / 2.0)
                .iter()
                .map(|p| {
                    if along_z {
                        [ax + p.0, w / 2.0 + p.1, at]
                    } else {
                        [at, w / 2.0 + p.1, az + p.0]
                    }
                })
                .collect();
            b.ring(&pts)
        };
        let (f0, f1) = (ring(&mut b, true, 0.0), ring(&mut b, true, t));
        let (g0, g1) = (ring(&mut b, false, 0.0), ring(&mut b, false, t));
        b.strip(&f0, &f1, false)?;
        b.strip(&g0, &g1, false)?;
        floor.push(Loop::new(f0, Role::Cut, hole_label));
        shelf.push(Loop::new(f1, Role::Cut, hole_label));
        back.push(Loop::new(g0, Role::Cut, hole_label));
        inner.push(Loop::new(g1, Role::Cut, hole_label));
    }
    b.plane(&floor, [0.0, 0.0, -1.0])?;
    b.plane(&shelf, [0.0, 0.0, 1.0])?;
    b.plane(&inner, [1.0, 0.0, 0.0])?;
    b.plane(&back, [-1.0, 0.0, 0.0])?;
    b.plane(&[Loop::new(end_a, Role::Boundary, label)], [1.0, 0.0, 0.0])?;
    b.plane(&[Loop::new(end_c, Role::Boundary, label)], [0.0, 0.0, 1.0])?;
    // the two flat sides of the angle, an L
    let side = |y: f64| -> Vec<P3> {
        [(0.0, 0.0), (a, 0.0), (a, t), (t, t), (t, c), (0.0, c)]
            .iter()
            .map(|p| [p.0, y, p.1])
            .collect()
    };
    let (front, rear) = (b.ring(&side(0.0)), b.ring(&side(w)));
    b.plane(&[Loop::new(front, Role::Boundary, label)], [0.0, -1.0, 0.0])?;
    b.plane(&[Loop::new(rear, Role::Boundary, label)], [0.0, 1.0, 0.0])?;
    Ok(b.finish())
}

/// The mesh of a part in the place the model gives it.
pub fn part_mesh(part: &PartInstance, circle: &Circle, limit: usize) -> Result<Mesh, CadError> {
    let size = part.part.size(part.cut);
    let fsize = [nm(size[0]), nm(size[1]), nm(size[2])];
    let local = match &part.part.shape {
        Shape::Profile { wall, .. } => profile_mesh(fsize, nm(wall.get()), limit)?,
        Shape::Screw {
            diameter,
            length,
            head,
            head_diameter,
            head_height,
            ..
        } => screw_mesh(
            circle,
            nm(diameter.get()),
            nm(length.get()),
            *head,
            nm(head_diameter.get()),
            nm(head_height.get()),
            limit,
        )?,
        Shape::Bracket {
            leg_a,
            leg_b,
            width,
            thickness,
            hole_diameter,
        } => bracket_mesh(
            circle,
            (nm(leg_a.get()), nm(leg_b.get())),
            nm(width.get()),
            nm(thickness.get()),
            nm(hole_diameter.get()),
            limit,
        )?,
    };
    let (_, shift) = part.extent();
    let cols = part.orientation.columns();
    let origin = [
        nm(part.at[0].get() + shift[0]),
        nm(part.at[1].get() + shift[1]),
        nm(part.at[2].get() + shift[2]),
    ];
    Ok(local.mapped(|p| {
        let mut g = origin;
        for (i, slot) in g.iter_mut().enumerate() {
            for (j, col) in cols.iter().enumerate() {
                *slot += f64::from(col[i]) * p[j];
            }
        }
        g
    }))
}

/// Checks that a mesh is closed and has the volume it must have.
pub fn verify(name: &str, mesh: &Mesh, expected_nm3: f64, ppm: u32) -> Result<(), CadError> {
    mesh.check_closed()
        .map_err(|why| CadError::MeshCheck(format!("{name}: {why}")))?;
    let got = mesh.volume_nm3();
    let off = ((got - expected_nm3) / expected_nm3.abs().max(1.0)).abs() * 1.0e6;
    if off > f64::from(ppm) {
        return Err(CadError::MeshCheck(format!(
            "{name}: the triangles hold {:.3} mm3 and the model {:.3} mm3 ({off:.0} parts per million apart, {ppm} allowed)",
            got / 1.0e18,
            expected_nm3 / 1.0e18
        )));
    }
    Ok(())
}

/// Every solid of the model: the box with everything on it and every placed part, each checked
/// against its exact volume.
pub fn bodies(enclosure: &Enclosure, settings: &MeshSettings) -> Result<Vec<Body>, CadError> {
    let circle = Circle::new(settings.circle_segments);
    let mut budget = settings.max_triangles as usize;
    let mut out = Vec::new();
    let mesh = enclosure_mesh(enclosure, &circle, budget)?;
    let expected = enclosure_volume_nm3(enclosure)?;
    verify("enclosure", &mesh, expected, settings.volume_tolerance_ppm)?;
    budget -= mesh.triangles.len();
    out.push(Body {
        name: "enclosure".to_owned(),
        mesh,
        expected_nm3: expected,
    });
    for part in enclosure.placed_parts().values() {
        let mesh = part_mesh(part, &circle, budget)?;
        let expected = part.volume_nm3()? as f64;
        verify(&part.name, &mesh, expected, settings.volume_tolerance_ppm)?;
        budget -= mesh.triangles.len();
        out.push(Body {
            name: part.name.clone(),
            mesh,
            expected_nm3: expected,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use crate::enclosure::{Edging, EnclosureParams, Operation};
    use crate::fins::{FinAxis, FinSet};
    use crate::lang::Lang;
    use crate::mount::{Base, BoreKind, LidFixing};
    use crate::sketch::{Sketch, SketchPlane};
    use crate::uiconfig::UiSettings;
    use crate::units::Count;
    use crate::window::Window;
    use crate::Length;
    use eh3d_container::ObjectId;
    use std::path::PathBuf;

    fn mm(v: i64) -> Length {
        Length::from_mm(v).unwrap()
    }

    fn um(v: i64) -> Length {
        Length::new(v * 1000).unwrap()
    }

    fn id(n: u64) -> ObjectId {
        ObjectId::from_parts(1_700_000_000_000, n as u16, n).unwrap()
    }

    fn model() -> Enclosure {
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

    fn add_hole(e: &mut Enclosure, n: u64, face: Face, u: i64, v: i64, d: i64, style: HoleStyle) {
        e.apply(Operation::AddHole {
            id: id(n),
            hole: Hole {
                name: format!("h{n}"),
                face,
                u: mm(u),
                v: mm(v),
                diameter: mm(d),
                style,
            },
        })
        .unwrap();
    }

    fn post(name: &str, x: i64, y: i64, base: Option<Base>) -> Post {
        Post {
            name: name.into(),
            board_hole: None,
            x: mm(x),
            y: mm(y),
            outer_diameter: mm(8),
            height: mm(6),
            kind: BoreKind::Insert,
            bore_diameter: um(4200),
            bore_depth: mm(5),
            base,
        }
    }

    fn settings() -> MeshSettings {
        Settings::defaults().mesh
    }

    /// The volume of the body of the box in cubic millimetres, after the checks.
    fn volume_mm3(e: &Enclosure) -> f64 {
        let b = bodies(e, &settings()).unwrap();
        b[0].mesh.volume_nm3() / 1.0e18
    }

    fn close(a: f64, b: f64, rel: f64) {
        assert!(((a - b) / b).abs() < rel, "{a} against {b}");
    }

    #[test]
    fn an_empty_box_is_two_closed_surfaces_with_the_volume_of_the_material() {
        let b = bodies(&model(), &settings()).unwrap();
        assert_eq!(b.len(), 1);
        // 12 triangles outside and 12 inside
        assert_eq!(b[0].mesh.triangles.len(), 24);
        assert_eq!(b[0].mesh.vertices.len(), 16);
        b[0].mesh.check_closed().unwrap();
        // 104 x 64 x 35 less 100 x 60 x 30
        close(b[0].mesh.volume_nm3() / 1.0e18, 52_960.0, 1e-12);
        let (lo, hi) = b[0].mesh.bounds().unwrap();
        assert_eq!((lo, hi), ([0.0, 0.0, 0.0], [104.0e6, 64.0e6, 35.0e6]));
    }

    #[test]
    fn a_plain_hole_takes_a_cylinder_of_the_thickness_of_the_wall() {
        let mut e = model();
        add_hole(&mut e, 1, Face::Front, 30, 17, 6, HoleStyle::Plain);
        // 52960 less pi * 3^2 * 2
        close(volume_mm3(&e), 52_903.451_332_235_38, 1e-12);
        let b = bodies(&e, &settings()).unwrap();
        // the wall of the hole is 64 quads, the two rings are 64 + 64 triangles with the rectangle
        assert!(b[0].mesh.triangles.len() > 24 + 128);
    }

    #[test]
    fn each_style_of_hole_has_the_volume_of_its_shape() {
        let cases = [
            (
                HoleStyle::Countersunk {
                    head_diameter: mm(8),
                },
                3,
                52_892.979_356_723_416,
            ),
            (
                HoleStyle::Counterbored {
                    head_diameter: mm(6),
                    depth: mm(1),
                },
                3,
                52_917.588_499_176_534,
            ),
            (HoleStyle::Rectangular { height: mm(8) }, 4, 52_864.0),
            (
                HoleStyle::Oval {
                    length: mm(10),
                    vertical: false,
                },
                4,
                52_850.300_888_156_926,
            ),
        ];
        for (style, d, expected) in cases {
            let mut e = model();
            add_hole(&mut e, 1, Face::Top, 50, 30, d, style);
            // the rectangle is exact; the slot has the polygon of a circle at the ends
            let rel = if matches!(style, HoleStyle::Oval { .. }) {
                2e-5
            } else {
                1e-11
            };
            close(volume_mm3(&e), expected, rel);
        }
    }

    #[test]
    fn the_holes_of_the_four_sides_the_lid_and_the_floor_close_the_surface() {
        let mut e = model();
        add_hole(&mut e, 1, Face::Front, 30, 17, 6, HoleStyle::Plain);
        add_hole(&mut e, 2, Face::Back, 30, 17, 6, HoleStyle::Plain);
        add_hole(&mut e, 3, Face::Left, 30, 17, 6, HoleStyle::Plain);
        add_hole(&mut e, 4, Face::Right, 30, 17, 6, HoleStyle::Plain);
        add_hole(&mut e, 5, Face::Top, 30, 30, 6, HoleStyle::Plain);
        add_hole(&mut e, 6, Face::Bottom, 30, 30, 6, HoleStyle::Plain);
        // 4 walls of 2 mm, the lid of 3 mm and the floor of 2 mm
        let expected = 52_960.0 - std::f64::consts::PI * 9.0 * (4.0 * 2.0 + 3.0 + 2.0);
        close(volume_mm3(&e), expected, 1e-12);
    }

    #[test]
    fn a_post_with_a_foot_and_a_bore_adds_its_volume() {
        let mut e = model();
        e.apply(Operation::AddPost {
            id: id(1),
            post: post("a", 25, 15, None),
        })
        .unwrap();
        // 8 mm over 6 mm less the bore of 4.2 mm over 5 mm
        close(volume_mm3(&e), 53_192.320_776_732_966, 1e-12);
        let mut e = model();
        let foot = Base {
            diameter: mm(12),
            height: mm(2),
        };
        e.apply(Operation::AddPost {
            id: id(1),
            post: post("a", 25, 15, Some(foot)),
        })
        .unwrap();
        close(volume_mm3(&e), 53_250.963_839_599_97, 1e-12);
    }

    #[test]
    fn cooling_fins_add_their_boxes() {
        let mut e = model();
        e.apply(Operation::AddFins {
            id: id(1),
            fins: FinSet {
                name: "sink".into(),
                face: Face::Top,
                along: FinAxis::U,
                count: Count::new(5).unwrap(),
                thickness: um(1200),
                height: mm(8),
                pitch: mm(8),
                first: mm(20),
                start: mm(10),
                length: mm(40),
            },
        })
        .unwrap();
        // 5 fins of 1.2 x 8 x 40 mm
        close(volume_mm3(&e), 54_880.0, 1e-12);
    }

    #[test]
    fn the_lip_of_the_lid_and_the_holes_over_the_posts_are_in_the_solid() {
        let mut e = model();
        for (n, x, y) in [(1u64, 25i64, 15i64), (2, 75, 15), (3, 25, 45), (4, 75, 45)] {
            e.apply(Operation::AddPost {
                id: id(n),
                post: post("p", x, y, None),
            })
            .unwrap();
        }
        e.apply(Operation::SetLid(Some(Lid {
            fixing: LidFixing::Auto { inset: mm(6) },
            screw: "M3".into(),
            hole_diameter: um(3400),
            style: HoleStyle::Countersunk {
                head_diameter: um(6300),
            },
            clearance: um(200),
            lip: Some(Lip {
                thickness: um(1200),
                depth: mm(3),
            }),
        })))
        .unwrap();
        let body = &bodies(&e, &settings()).unwrap()[0];
        body.mesh.check_closed().unwrap();
        let expected = enclosure_volume_nm3(&e).unwrap();
        close(body.mesh.volume_nm3(), expected, 1e-9);
        assert!(expected > 52_960.0e18 + 4.0 * 232.0e18);
    }

    #[test]
    fn rounded_and_chamfered_vertical_edges_keep_the_surface_closed() {
        for t in [Treatment::Round(um(1500)), Treatment::Chamfer(um(1500))] {
            let mut e = model();
            e.apply(Operation::SetEdging(Edging {
                vertical: t,
                ..Default::default()
            }))
            .unwrap();
            let body = &bodies(&e, &settings()).unwrap()[0];
            body.mesh.check_closed().unwrap();
            let exact = 52_960.0 - e.edging_volume().unwrap().nm3 as f64 / 1.0e18;
            // a chamfer is exact; the sides of the arc are inside it
            let rel = if matches!(t, Treatment::Chamfer(_)) {
                1e-12
            } else {
                2e-5
            };
            close(body.mesh.volume_nm3() / 1.0e18, exact, rel);
        }
    }

    #[test]
    fn what_the_solid_cannot_be_made_of_is_refused_with_the_shape_named() {
        let mut e = model();
        e.apply(Operation::SetEdging(Edging {
            top: Treatment::Chamfer(um(1500)),
            ..Default::default()
        }))
        .unwrap();
        let err = bodies(&e, &settings()).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-048");
        assert!(err.to_string().contains("lid or the floor"), "{err}");
        // an extrusion that starts on the edge of its face
        let mut e = model();
        e.set_sketches(vec![plate("plate", 0)], None);
        e.apply(Operation::Extrude {
            id: id(1),
            sketch: "plate".into(),
            height: mm(5),
        })
        .unwrap();
        let err = bodies(&e, &settings()).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-048");
        assert!(
            err.to_string().contains("the lid and the sketch plate"),
            "{err}"
        );
        // a post a millimetre from the wall is made
        let mut e = model();
        e.apply(Operation::AddPost {
            id: id(1),
            post: post("near", 5, 15, None),
        })
        .unwrap();
        assert!(bodies(&e, &settings()).is_ok());
    }

    #[test]
    fn the_triangles_are_the_same_every_time_and_the_limit_is_a_refusal() {
        let mut e = model();
        add_hole(&mut e, 1, Face::Front, 30, 17, 6, HoleStyle::Plain);
        let a = bodies(&e, &settings()).unwrap();
        let b = bodies(&e, &settings()).unwrap();
        assert_eq!(a[0].mesh, b[0].mesh);
        let mut small = settings();
        small.max_triangles = 100;
        assert_eq!(bodies(&e, &small).unwrap_err().code(), "PHOENIX-CAD-050");
        // fewer sides make fewer triangles and a volume that is still the exact one
        let mut coarse = settings();
        coarse.circle_segments = 16;
        let c = bodies(&e, &coarse).unwrap();
        assert!(c[0].mesh.triangles.len() < a[0].mesh.triangles.len());
        close(
            c[0].mesh.volume_nm3() / 1.0e18,
            52_903.451_332_235_38,
            1e-12,
        );
    }

    #[test]
    fn a_mesh_that_does_not_have_the_volume_of_the_model_is_refused() {
        let mut e = model();
        add_hole(&mut e, 1, Face::Front, 30, 17, 6, HoleStyle::Plain);
        let body = &bodies(&e, &settings()).unwrap()[0];
        let err = verify("enclosure", &body.mesh, body.expected_nm3 * 1.01, 2000).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-049");
        assert!(err.to_string().contains("parts per million"), "{err}");
        let mut open = body.mesh.clone();
        open.triangles.pop();
        let err = verify("enclosure", &open, body.expected_nm3, 2000).unwrap_err();
        assert!(err.to_string().contains("open edges"), "{err}");
    }

    fn open_example(name: &str) -> Window {
        let path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/"))
            .join(format!("{name}.phoenixproj"));
        let mut w = Window::new(Lang::En, Settings::defaults(), UiSettings::defaults()).unwrap();
        assert!(w.open(&path), "{name} {:?}", w.status);
        w
    }

    #[test]
    fn the_example_box_with_its_posts_lip_and_rounded_corners_is_a_closed_solid() {
        let w = open_example("box");
        let b = bodies(w.enclosure(), &w.settings.mesh).unwrap();
        assert_eq!(b.len(), 1);
        b[0].mesh.check_closed().unwrap();
        assert_eq!(b[0].mesh.flat_triangles(), 0);
        // the polygons of the rounded corners are inside the arcs by a few parts per million
        close(b[0].mesh.volume_nm3(), b[0].expected_nm3, 5e-5);
    }

    #[test]
    fn the_parts_of_the_frame_are_closed_solids_with_the_volume_of_the_catalog() {
        let w = open_example("frame");
        let b = bodies(w.enclosure(), &w.settings.mesh).unwrap();
        // the box and 8 bars, 4 brackets, 4 bolts and 2 screws
        assert_eq!(b.len(), 1 + 8 + 4 + 4 + 2);
        for body in &b {
            body.mesh.check_closed().unwrap();
            close(body.mesh.volume_nm3(), body.expected_nm3, 1e-9);
        }
        // a bar of 40 x 20 mm with a wall of 2 mm, 300 mm long: (800 - 36 * 16 ... ) = 67200 mm3
        let rail = b.iter().find(|x| x.name == "front rail").unwrap();
        close(rail.mesh.volume_nm3() / 1.0e18, 67_200.0, 1e-12);
        let (lo, hi) = rail.mesh.bounds().unwrap();
        assert_eq!((lo, hi), ([0.0, 0.0, 0.0], [300.0e6, 40.0e6, 20.0e6]));
        // a bracket of 3324.574 mm3: the two legs less one hole in each
        let bracket = b.iter().find(|x| x.name == "bracket 1").unwrap();
        close(
            bracket.mesh.volume_nm3() / 1.0e18,
            3_324.574_123_147_204,
            1e-9,
        );
    }

    #[test]
    fn the_example_assembly_is_a_box_and_six_parts() {
        let w = open_example("assembly");
        let b = bodies(w.enclosure(), &w.settings.mesh).unwrap();
        assert_eq!(b.len(), 1 + 6);
        let total: usize = b.iter().map(|x| x.mesh.triangles.len()).sum();
        assert!(total > 1000);
    }

    /// A sketch on the lid with the origin at `u0`, 10 mm: a rectangle of 40 by 25 mm and a
    /// circle of diameter 6 mm in it.
    fn plate(name: &str, u0: i64) -> Sketch {
        let plane = SketchPlane {
            face: Face::Top,
            u0: mm(u0),
            v0: mm(10),
            offset: Length::ZERO,
        };
        let mut k = Sketch::new(name, plane).unwrap();
        k.add_rectangle((mm(0), mm(0)), (mm(40), mm(25))).unwrap();
        let c = k.add_point(mm(20), mm(12)).unwrap();
        k.add_circle(c, mm(3)).unwrap();
        k
    }

    fn with_plate(op: Operation) -> Enclosure {
        let mut e = model();
        e.set_sketches(vec![plate("plate", 10)], None);
        e.apply(op).unwrap();
        e
    }

    #[test]
    fn an_extrusion_with_a_round_hole_adds_the_area_of_its_outline_times_its_height() {
        let e = with_plate(Operation::Extrude {
            id: id(1),
            sketch: "plate".into(),
            height: mm(5),
        });
        let body = &bodies(&e, &settings()).unwrap()[0];
        body.mesh.check_closed().unwrap();
        // 40 x 25 less a circle of 3 mm radius, 5 mm high
        let expected = 52_960.0 + (1000.0 - std::f64::consts::PI * 9.0) * 5.0;
        close(body.expected_nm3 / 1.0e18, expected, 1e-9);
        close(body.mesh.volume_nm3() / 1.0e18, expected, 1e-3);
        let (_, hi) = body.mesh.bounds().unwrap();
        assert_eq!(hi[2], 40.0e6);
    }

    #[test]
    fn a_pocket_takes_the_outline_out_of_the_lid_and_leaves_its_island() {
        let area = 1000.0 - std::f64::consts::PI * 9.0;
        let shallow = with_plate(Operation::Pocket {
            id: id(1),
            sketch: "plate".into(),
            depth: Some(mm(1)),
        });
        let body = &bodies(&shallow, &settings()).unwrap()[0];
        body.mesh.check_closed().unwrap();
        close(body.mesh.volume_nm3() / 1.0e18, 52_960.0 - area, 1e-3);
        // through the whole lid of 3 mm: the circle stays as a column
        let through = with_plate(Operation::Pocket {
            id: id(1),
            sketch: "plate".into(),
            depth: None,
        });
        let body = &bodies(&through, &settings()).unwrap()[0];
        body.mesh.check_closed().unwrap();
        close(body.mesh.volume_nm3() / 1.0e18, 52_960.0 - area * 3.0, 1e-3);
    }

    #[test]
    fn an_extrusion_on_the_edge_of_its_face_is_refused_and_says_which() {
        // the plate of the example starts at the corner of the lid and covers the holes of its screws
        let w = open_example("plate");
        let err = bodies(w.enclosure(), &w.settings.mesh).unwrap_err();
        assert_eq!(err.code(), "PHOENIX-CAD-048");
        assert!(err.to_string().contains("lid"), "{err}");
    }
}
