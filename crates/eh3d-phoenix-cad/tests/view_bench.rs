// SPDX-License-Identifier: GPL-3.0-or-later
//! The times of the view of a large file, measured on a made-up sphere. It is left out of the
//! ordinary run; to take the times:
//!
//! `cargo test --release -p eh3d-phoenix-cad --test view_bench -- --ignored --nocapture`

use std::time::Instant;

use eh3d_phoenix_cad::config::Settings;
use eh3d_phoenix_cad::lang::Lang;
use eh3d_phoenix_cad::mesh::{stl_binary, Mesh};
use eh3d_phoenix_cad::modelview::{self, Content, Look, Pose, ViewOptions};
use eh3d_phoenix_cad::projection::Camera;
use eh3d_phoenix_cad::sketch::settings::SketchSettings;
use eh3d_phoenix_cad::uiconfig::UiSettings;

/// A closed sphere of 40 mm cut into `around` slices and `down` stacks.
fn sphere(around: u32, down: u32) -> Mesh {
    let radius = 20.0e6;
    let mut vertices = vec![[0.0, 0.0, radius]];
    for stack in 1..down {
        let polar = std::f64::consts::PI * f64::from(stack) / f64::from(down);
        for slice in 0..around {
            let turn = std::f64::consts::TAU * f64::from(slice) / f64::from(around);
            vertices.push([
                (radius * polar.sin() * turn.cos()).round(),
                (radius * polar.sin() * turn.sin()).round(),
                (radius * polar.cos()).round(),
            ]);
        }
    }
    let south = vertices.len() as u32;
    vertices.push([0.0, 0.0, -radius]);
    let at = |stack: u32, slice: u32| 1 + (stack - 1) * around + slice % around;
    let mut triangles = Vec::new();
    for slice in 0..around {
        triangles.push([0, at(1, slice), at(1, slice + 1)]);
        for stack in 1..down - 1 {
            let (a, b) = (at(stack, slice), at(stack, slice + 1));
            let (c, d) = (at(stack + 1, slice), at(stack + 1, slice + 1));
            triangles.push([a, c, d]);
            triangles.push([a, d, b]);
        }
        triangles.push([at(down - 1, slice + 1), at(down - 1, slice), south]);
    }
    Mesh {
        vertices,
        triangles,
    }
}

#[test]
#[ignore = "a measurement, run by hand"]
fn the_times_of_the_view_of_a_large_file() {
    let settings = Settings::defaults();
    let sketch = SketchSettings::defaults();
    for (around, down) in [(200, 101), (500, 251), (1000, 501)] {
        let mesh = sphere(around, down);
        let bytes = stl_binary(&[("sphere", &mesh)]);
        let mut options = ViewOptions::new(&settings, &UiSettings::defaults().model_view, &sketch);
        options.max_triangles = 2_000_000;
        let started = Instant::now();
        let content = modelview::load("sphere.stl", &bytes, &options, Lang::En).unwrap();
        let loaded = started.elapsed();
        let Content::Solid(solid) = content else {
            panic!("not a solid")
        };
        let pose = Pose::fitted(Camera {
            yaw: 35.0,
            pitch: 28.0,
        });
        let look = Look {
            edges: true,
            ..Look::default()
        };
        let started = Instant::now();
        for _ in 0..5 {
            let picture = modelview::render(&solid, &pose, 1600, 1000, look);
            assert_eq!(picture.width, 1600);
        }
        let drawn = started.elapsed() / 5;
        println!(
            "VIEW_BENCH triangles={} load_ms={} picture_ms={}",
            solid.count,
            loaded.as_millis(),
            drawn.as_millis()
        );
    }
}
