// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests of the view of a file of another program. Every file here is made up.

use std::time::{Duration, Instant};

use eh3d_attachpanel::{Model, Program, Request};

use super::*;
use crate::eh3dfile::{tests::Scratch, SaveSettings};
use crate::step_faceted::{faceted_step, Schema};

/// A cube of 10 mm with a corner at the origin.
fn cube() -> Mesh {
    let s = 10.0 * NM_PER_MM;
    let mut vertices = Vec::new();
    for z in [0.0, s] {
        for y in [0.0, s] {
            for x in [0.0, s] {
                vertices.push([x, y, z]);
            }
        }
    }
    let triangles = vec![
        [0, 2, 1],
        [1, 2, 3],
        [4, 5, 6],
        [5, 7, 6],
        [0, 1, 4],
        [1, 5, 4],
        [2, 6, 3],
        [3, 6, 7],
        [0, 4, 2],
        [2, 4, 6],
        [1, 3, 5],
        [3, 7, 5],
    ];
    Mesh {
        vertices,
        triangles,
    }
}

fn cube_stl() -> Vec<u8> {
    crate::mesh::stl_binary(&[("cube", &cube())])
}

fn options() -> ViewOptions {
    ViewOptions::new(
        &Settings::defaults(),
        &crate::uiconfig::UiSettings::defaults().model_view,
        &SketchSettings::defaults(),
    )
}

fn solid(content: Content) -> Solid {
    match content {
        Content::Solid(solid) => solid,
        Content::Flat(flat) => panic!("a flat drawing: {flat:?}"),
    }
}

fn cube_solid() -> Solid {
    solid(load("cube.stl", &cube_stl(), &options(), Lang::En).unwrap())
}

const PLAIN: Look = Look {
    edges: false,
    cut: None,
};

const LINED: Look = Look {
    edges: true,
    cut: None,
};

const FLAT_DXF: &str = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n30\n21\n0\n\
    0\nCIRCLE\n8\n0\n10\n10\n20\n0\n40\n5\n0\nENDSEC\n0\nEOF\n";

#[test]
fn the_files_that_are_shown_are_told_by_their_extension() {
    for name in [
        "a.stl", "b.STEP", "c.stp", "d.obj", "e.3mf", "f.ply", "g.off", "h.Dxf",
    ] {
        assert!(can_show(name), "{name}");
    }
    for name in ["a.pdf", "stl", "b.step.exe", "c.iges", ""] {
        assert!(!can_show(name), "{name}");
    }
}

#[test]
fn a_cube_is_read_with_its_size_its_volume_and_its_twelve_edges() {
    let cube = cube_solid();
    assert_eq!((cube.bodies, cube.count, cube.triangles.len()), (1, 12, 12));
    assert_eq!(cube.size(), [10.0, 10.0, 10.0]);
    assert!(cube.closed && cube.over_limit.is_none());
    assert_eq!(cube.open_edges, 0);
    assert!((cube.volume_mm3.unwrap() - 1000.0).abs() < 1e-6);
    // each edge of the cube is a side of two triangles; the diagonals of the faces are not sharp
    let marked: u32 = cube
        .triangles
        .iter()
        .map(|t| u32::from(t.edges).count_ones())
        .sum();
    assert_eq!(marked, 24);
    let said = facts(&Content::Solid(cube), Some([20.0, 20.0, 20.0]), Lang::En);
    assert_eq!(said[0], "Solids: 1. Triangles: 12.");
    assert_eq!(said[1], "Size: 10 x 10 x 10 mm");
    assert_eq!(said[2], "Volume: 1 cm3");
    assert_eq!(said[3], "Fits inside the box (20 x 20 x 20 mm inside).");
    assert_eq!(said.len(), 4);
}

#[test]
fn a_part_that_is_larger_than_the_inside_of_the_box_is_told() {
    let content = Content::Solid(cube_solid());
    let said = facts(&content, Some([20.0, 5.0, 20.0]), Lang::En);
    assert!(
        said[3].starts_with("Does not fit inside the box"),
        "{said:?}"
    );
    // without a box nothing is said about it
    assert_eq!(facts(&content, None, Lang::En).len(), 3);
}

#[test]
fn a_step_file_of_the_cube_is_shown_as_the_same_solid() {
    let text = faceted_step(&[("cube", &cube())], Schema::Ap214).unwrap();
    let read = solid(load("part.step", text.as_bytes(), &options(), Lang::En).unwrap());
    assert_eq!(read.size(), [10.0, 10.0, 10.0]);
    assert!(read.closed);
}

#[test]
fn an_open_surface_has_no_volume_and_says_how_many_edges_are_open() {
    let mut open = cube();
    open.triangles.pop();
    let bytes = crate::mesh::stl_binary(&[("open", &open)]);
    let read = solid(load("open.stl", &bytes, &options(), Lang::En).unwrap());
    assert!(!read.closed);
    assert_eq!((read.open_edges, read.volume_mm3), (3, None));
    let said = facts(&Content::Solid(read.clone()), None, Lang::En);
    assert!(said[2].ends_with("(open edges: 3)."), "{said:?}");
    // an open surface is drawn from both sides: nothing of its outline is left empty
    let below = Camera {
        yaw: 0.0,
        pitch: -90.0,
    };
    let picture = render(&read, &Pose::fitted(below), 400, 400, PLAIN);
    assert_eq!(picture.pixel(200, 200)[3], 255);
}

#[test]
fn more_triangles_than_can_be_drawn_are_told_about_and_not_drawn() {
    let mut few = options();
    few.max_triangles = 5;
    let read = solid(load("cube.stl", &cube_stl(), &few, Lang::En).unwrap());
    assert!(read.triangles.is_empty());
    assert_eq!((read.count, read.over_limit), (12, Some(5)));
    assert_eq!(read.size(), [10.0, 10.0, 10.0]);
    let said = facts(&Content::Solid(read), None, Lang::En);
    assert!(
        said.iter()
            .any(|l| l.contains("12 triangles") && l.ends_with("is 5.")),
        "{said:?}"
    );
}

#[test]
fn what_cannot_be_read_is_refused_with_words() {
    let mut small = options();
    small.max_bytes = 100;
    let said = load("cube.stl", &cube_stl(), &small, Lang::En).unwrap_err();
    assert!(said.contains("model_view.max_mb"), "{said}");
    let said = load("sheet.pdf", b"%PDF", &options(), Lang::En).unwrap_err();
    assert!(said.starts_with("Only STL"), "{said}");
    assert!(load("junk.stl", b"junk", &options(), Lang::En).is_err());
    assert!(load("junk.step", b"junk", &options(), Lang::En).is_err());
    assert!(load("junk.dxf", &[0xff, 0xfe, 0x00], &options(), Lang::En).is_err());
}

#[test]
fn a_flat_drawing_is_read_with_its_size_and_fits_the_pane() {
    let content = load("plate.dxf", FLAT_DXF.as_bytes(), &options(), Lang::En).unwrap();
    let Content::Flat(flat) = &content else {
        panic!("a solid from a flat drawing");
    };
    // the line is 30 mm long and the circle of 5 mm of radius sits on it
    assert!((flat.size[0] - 30.0).abs() < 1e-6, "{:?}", flat.size);
    assert!((flat.size[1] - 10.0).abs() < 1e-6, "{:?}", flat.size);
    let said = facts(&content, Some([1.0, 1.0, 1.0]), Lang::En);
    assert_eq!(said[0], "Flat drawing: 30 x 10 mm");
    let pose = Pose::fitted(Camera::TOP);
    let lines = flat_lines(flat, &pose, (324.0, 224.0));
    assert!(lines.iter().any(|l| l.2) && lines.iter().any(|l| !l.2));
    let ends: Vec<(f64, f64)> = lines.iter().flat_map(|l| [l.0, l.1]).collect();
    let across = ends.iter().map(|p| p.0);
    // the wide side takes the pane but for its margin of 12 pixels
    assert!((across.clone().fold(f64::INFINITY, f64::min) - 12.0).abs() < 0.5);
    assert!((across.fold(0.0, f64::max) - 312.0).abs() < 0.5);
    assert!(ends.iter().all(|p| p.1 >= 11.5 && p.1 <= 212.5));
}

#[test]
fn the_picture_shows_the_lit_face_that_is_seen_and_nothing_round_it() {
    let cube = cube_solid();
    let picture = render(&cube, &Pose::fitted(Camera::TOP), 400, 400, PLAIN);
    assert_eq!((picture.width, picture.height), (400, 400));
    // from above the top face fills the middle: 150 of red times a shade of about 0.816
    for (x, y) in [(200, 200), (120, 120), (280, 280)] {
        let [red, _, _, alpha] = picture.pixel(x, y);
        assert!((121..=123).contains(&red), "{red}");
        assert_eq!(alpha, 255);
    }
    // outside the cube nothing is drawn
    assert_eq!(picture.pixel(2, 2), [0, 0, 0, 0]);
    assert_eq!(picture.pixel(1000, 1000), [0, 0, 0, 0]);
    // from a corner three faces of different light are seen
    let corner = Camera {
        yaw: 45.0,
        pitch: 30.0,
    };
    let picture = render(&cube, &Pose::fitted(corner), 400, 400, PLAIN);
    let shades: std::collections::BTreeSet<u8> = picture
        .rgba
        .chunks(4)
        .filter(|p| p[3] == 255)
        .map(|p| p[0])
        .collect();
    assert_eq!(shades.len(), 3, "{shades:?}");
}

#[test]
fn what_is_behind_is_hidden_whatever_the_order_and_the_size_of_the_triangles() {
    // a small flat square near the person and a much larger leaning one under it
    let square = |half: f64, z: f64, lean: f64| -> Vec<[f64; 9]> {
        let a = [-half, -half, z];
        let b = [half, -half, z];
        let c = [half, half, z + lean];
        let d = [-half, half, z + lean];
        [[a, b, c], [a, c, d]]
            .into_iter()
            .map(|[p, q, r]| [p[0], p[1], p[2], q[0], q[1], q[2], r[0], r[1], r[2]])
            .collect()
    };
    let near = square(2.0, 5.0, 0.0);
    // its mean depth says nothing about what it hides, as it leans so much
    let far = square(20.0, -20.0, 24.0);
    let picture_of = |triangles: Vec<[f64; 9]>| {
        let mesh = interchange::mesh_from_triangles(&triangles);
        let bytes = crate::mesh::stl_binary(&[("two", &mesh)]);
        let read = solid(load("two.stl", &bytes, &options(), Lang::En).unwrap());
        render(&read, &Pose::fitted(Camera::TOP), 200, 200, PLAIN)
    };
    let one = picture_of([near.clone(), far.clone()].concat());
    let other = picture_of([far, near].concat());
    assert_eq!(one.rgba, other.rgba);
    // the near square is flat, so it has the shade of a face seen from above
    let red = one.pixel(100, 100)[0];
    assert!((121..=123).contains(&red), "{red}");
    assert_ne!(one.pixel(100, 130)[0], red);
}

#[test]
fn the_sharp_edges_are_drawn_when_asked_and_not_where_they_are_hidden() {
    let cube = cube_solid();
    let pose = Pose::fitted(Camera::TOP);
    let plain = render(&cube, &pose, 400, 400, PLAIN);
    let lined = render(&cube, &pose, 400, 400, LINED);
    let dark = |p: &Picture| p.rgba.chunks(4).filter(|c| *c == [10, 20, 28, 255]).count();
    assert_eq!(dark(&plain), 0);
    assert!(dark(&lined) > 400, "{}", dark(&lined));
    // the diagonal of the top face is not an edge, and the bottom is not seen through it
    assert_eq!(lined.pixel(200, 200), plain.pixel(200, 200));
}

#[test]
fn the_corner_under_the_pointer_is_a_corner_that_is_not_hidden() {
    let cube = cube_solid();
    let pose = Pose::fitted(Camera::TOP);
    let picture = render(&cube, &pose, 400, 400, LINED);
    let top_corner = [10.0, 10.0, 10.0];
    let at = place_of(&cube, &pose, (400.0, 400.0), top_corner);
    // the corner under it is on the same pixel and farther: the near one is picked
    assert_eq!(pick(&cube, &pose, &picture, at, 6.0), Some(top_corner));
    assert_eq!(pick(&cube, &pose, &picture, (2.0, 2.0), 6.0), None);
    // from a corner view the far corner of the cube is behind it and cannot be picked
    let corner = Pose::fitted(Camera {
        yaw: 45.0,
        pitch: 30.0,
    });
    let picture = render(&cube, &corner, 400, 400, LINED);
    let far = [10.0, 10.0, 0.0];
    let at = place_of(&cube, &corner, (400.0, 400.0), far);
    assert_ne!(pick(&cube, &corner, &picture, at, 3.0), Some(far));
    assert_eq!(
        distance_text([0.0; 3], [3.0, 4.0, 0.0], Lang::En),
        "Distance 5 mm (x 3, y 4, z 0 mm)"
    );
}

#[test]
fn a_cut_shows_the_inside_in_a_colour_of_its_own_and_hides_what_is_beyond() {
    let cube = cube_solid();
    assert_eq!(Cut::of(&cube, 2, 0.5), Cut { axis: 2, at: 5.0 });
    // the share and the axis stay inside what there is
    assert_eq!(Cut::of(&cube, 9, 7.0), Cut { axis: 2, at: 10.0 });
    assert_eq!(Cut::of(&cube, 0, -1.0), Cut { axis: 0, at: 0.0 });
    let pose = Pose::fitted(Camera::TOP);
    let cut = |share: f64| Look {
        edges: true,
        cut: Some(Cut::of(&cube, 2, share)),
    };
    // cut under its top, from above the inside of the bottom is seen: warm, not blue
    let open = render(&cube, &pose, 400, 400, cut(0.5));
    let [red, _, blue, alpha] = open.pixel(200, 150);
    assert_eq!(alpha, 255);
    assert!(red > blue, "{red} {blue}");
    // a cut at the far side keeps the top, which is blue
    let whole = render(&cube, &pose, 400, 400, cut(1.0));
    let [red, _, blue, _] = whole.pixel(200, 150);
    assert!(blue > red, "{red} {blue}");
    // a corner that was cut away is not picked: the one under it is
    let top_corner = [10.0, 10.0, 10.0];
    let at = place_of(&cube, &pose, (400.0, 400.0), top_corner);
    assert_eq!(pick(&cube, &pose, &open, at, 6.0), Some([10.0, 10.0, 0.0]));
    // along x, what is left of the cube ends at the cut
    let side = Look {
        edges: false,
        cut: Some(Cut::of(&cube, 0, 0.5)),
    };
    let half = render(&cube, &pose, 400, 400, side);
    assert_eq!(half.pixel(150, 200)[3], 255);
    assert_eq!(half.pixel(250, 200)[3], 0);
}

#[test]
fn turning_and_zooming_stay_within_their_bounds() {
    let mut pose = Pose::fitted(Camera::TOP);
    pose.turn(-30.0, 50.0);
    assert_eq!((pose.camera.yaw, pose.camera.pitch), (330.0, 90.0));
    pose.turn(60.0, -500.0);
    assert_eq!((pose.camera.yaw, pose.camera.pitch), (30.0, -90.0));
    pose.zoom_by(1e9);
    assert_eq!(pose.zoom, 50.0);
    pose.zoom_by(1e-9);
    assert_eq!(pose.zoom, 0.05);
    pose.zoom_by(f64::NAN);
    pose.zoom_by(-2.0);
    assert_eq!(pose.zoom, 0.05);
}

fn finished(job: &Job) -> Result<Content, String> {
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(done) = job.poll() {
            return done;
        }
        assert!(Instant::now() < until, "the reading did not end");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_file_of_the_disk_is_read_on_a_thread_of_its_own() {
    let dir = Scratch::new("view-disk");
    let path = dir.path("cube.stl");
    std::fs::write(&path, cube_stl()).unwrap();
    let origin = Origin::File(path);
    assert_eq!(origin.name(), "cube.stl");
    let job = Job::start(origin, options(), Lang::En);
    assert_eq!(solid(finished(&job).unwrap()).count, 12);
    // a larger file than the settings allow is not read whole
    let mut small = options();
    small.max_bytes = 100;
    let job = Job::start(Origin::File(dir.path("cube.stl")), small, Lang::En);
    assert!(job.wait().unwrap_err().contains("model_view.max_mb"));
    let job = Job::start(Origin::File(dir.path("gone.stl")), options(), Lang::En);
    assert!(job.wait().is_err());
    let job = Job::start(Origin::File(dir.path("gone.pdf")), options(), Lang::En);
    assert!(job.wait().unwrap_err().starts_with("Only STL"));
}

#[test]
fn an_attachment_of_the_project_is_shown_without_taking_it_out() {
    let dir = Scratch::new("view-attachment");
    let project = dir.path("p.eh3d");
    crate::eh3dfile::save_cad_project(&project, b"project", &SaveSettings::default()).unwrap();
    let part = dir.path("connector.stl");
    std::fs::write(&part, cube_stl()).unwrap();
    let mut panel = Model::new(Program::new("cad", "0.1.0"));
    panel.set_project(Some(&project));
    panel.request(Request::AttachFile(part.clone()));
    let until = Instant::now() + Duration::from_secs(60);
    while panel.busy() {
        assert!(Instant::now() < until, "the attachment did not end");
        std::thread::sleep(Duration::from_millis(5));
        panel.poll();
    }
    std::fs::remove_file(&part).unwrap();
    let item = panel.selected().cloned().expect("the new attachment");
    assert!(can_show(&item.name));
    let source = panel.source(&item.id).unwrap().unwrap();
    let origin = Origin::Attachment(Box::new(source));
    assert_eq!(origin.name(), "connector.stl");
    let job = Job::start(origin, options(), Lang::En);
    assert_eq!(solid(finished(&job).unwrap()).size(), [10.0, 10.0, 10.0]);
}

#[test]
fn the_original_that_an_import_kept_is_read_back_to_look_at_it() {
    let dir = Scratch::new("view-original");
    let project = dir.path("p.eh3d");
    crate::eh3dfile::save_cad_project(&project, b"project", &SaveSettings::default()).unwrap();
    assert_eq!(
        crate::eh3dfile::kept_original(&project, 1 << 20).unwrap(),
        None
    );
    let part = dir.path("vendor.stl");
    std::fs::write(&part, cube_stl()).unwrap();
    crate::suitewin::import_original(
        &project,
        &part,
        &Settings::defaults().mesh,
        &SaveSettings::default(),
    )
    .map_err(|e| e.text(Lang::En))
    .unwrap();
    let (name, bytes) = crate::eh3dfile::kept_original(&project, 1 << 20)
        .unwrap()
        .expect("an original");
    assert_eq!((name.as_str(), &bytes), ("vendor.stl", &cube_stl()));
    assert!(crate::eh3dfile::kept_original(&project, 10).is_err());
    let job = Job::start(Origin::Bytes(name, bytes), options(), Lang::En);
    assert_eq!(solid(job.wait().unwrap()).count, 12);
}
