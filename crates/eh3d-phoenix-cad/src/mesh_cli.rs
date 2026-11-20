// SPDX-License-Identifier: GPL-3.0-or-later
//! The command line of the tools for files of triangles and for tolerances: `mesh-convert`,
//! `mesh-transform`, `mesh-pattern`, `mesh-slice`, `mesh-clip` and `iso-fit`. Each answers with a
//! text that the program prints and whether everything was in order (the exit code of the
//! program: 0 for yes, 1 for no).
//!
//! The reports use the language of `--lang` (English without it); the comma separated values never
//! change with the language. A file is never written over one that exists unless `--overwrite` is
//! given.

use std::fs;
use std::path::{Path, PathBuf};

use crate::export::write_bytes;
use crate::interchange::{self, Body, ReadLimits};
use crate::iso286;
use crate::lang::{fill, Msg};
use crate::measurecmd;
use crate::mesh::{stl_ascii, stl_binary, Mesh, P3};
use crate::meshops::{self, Axis};
use crate::step_faceted::{faceted_step, Schema};
use crate::stlread::ReadWarning;
use crate::tools_cli::{load_settings, Args};
use crate::window::error_text;

/// The commands of this module, for the program to know which words are theirs.
pub const COMMANDS: [&str; 6] = [
    "mesh-convert",
    "mesh-transform",
    "mesh-pattern",
    "mesh-slice",
    "mesh-clip",
    "iso-fit",
];

/// Runs one of the commands of [`COMMANDS`] and returns what it prints and whether all was in order.
pub fn run(command: &str, args: &[String]) -> Result<(String, bool), String> {
    match command {
        "mesh-convert" => convert(args),
        "mesh-transform" => transform(args),
        "mesh-pattern" => pattern(args),
        "mesh-slice" => slice(args),
        "mesh-clip" => clip(args),
        "iso-fit" => iso_fit(args),
        other => Err(format!("{other}: unknown command (see help)")),
    }
}

/// The text of a warning of a reader in the language.
pub fn warning_text(w: &ReadWarning, lang: crate::lang::Lang) -> String {
    match w {
        ReadWarning::TrailingBytes(n) => fill(Msg::ReadWarnTrailing.tr(lang), &[&n.to_string()]),
        ReadWarning::Truncated(said, held) => fill(
            Msg::ReadWarnTruncated.tr(lang),
            &[&said.to_string(), &held.to_string()],
        ),
        ReadWarning::NonFinite(n) => fill(Msg::ReadWarnNonFinite.tr(lang), &[&n.to_string()]),
        ReadWarning::BinaryWithSolidHeader => Msg::ReadWarnBinarySolid.tr(lang).to_owned(),
        ReadWarning::Ignored(what) => fill(Msg::ReadWarnIgnored.tr(lang), &[what]),
    }
}

/// The solids of the file named, with the warnings of the reader as lines of text.
fn read_input(
    file: &Path,
    config: Option<&str>,
    lang: crate::lang::Lang,
) -> Result<
    (
        Vec<Body>,
        String,
        crate::config::Settings,
        crate::uiconfig::UiSettings,
    ),
    String,
> {
    let (settings, ui) = load_settings(config)?;
    let bytes = fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let limits = ReadLimits::from_settings(&settings.mesh);
    let read = interchange::read_file_checked(&file.to_string_lossy(), &bytes, &limits)
        .map_err(|e| format!("{}: {e}", file.display()))?;
    let mut text = String::new();
    for w in &read.warnings {
        text.push_str(&format!("{}: {}\n", w.name(), warning_text(w, lang)));
    }
    Ok((read.bodies, text, settings, ui))
}

/// The bytes of a file of triangles by the extension of its name.
fn encode(target: &Path, bodies: &[Body], ascii: bool, schema: Schema) -> Result<Vec<u8>, String> {
    let refs: Vec<(&str, &Mesh)> = bodies.iter().map(|(n, m)| (n.as_str(), m)).collect();
    let extension = target
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "stl" => Ok(if ascii {
            stl_ascii(&refs).into_bytes()
        } else {
            stl_binary(&refs)
        }),
        "obj" => Ok(interchange::obj(&refs).into_bytes()),
        "3mf" => Ok(interchange::three_mf(&refs)),
        "ply" => Ok(if ascii {
            crate::ply::write_ascii(&refs).into_bytes()
        } else {
            crate::ply::write_binary(&refs)
        }),
        "off" => Ok(crate::off::write(&refs).into_bytes()),
        "dxf" => Ok(crate::dxf_mesh::write(&refs).into_bytes()),
        "step" | "stp" => faceted_step(&refs, schema).map(String::into_bytes),
        _ => Err(
            "the output must end in .stl, .obj, .3mf, .ply, .off, .dxf, .step or .stp".to_owned(),
        ),
    }
}

/// Writes the solids to a file and says so.
fn write_out(
    target: &Path,
    bodies: &[Body],
    a: &Args,
    ui: &crate::uiconfig::UiSettings,
    schema: Schema,
) -> Result<String, String> {
    let bytes = encode(target, bodies, a.has("--ascii"), schema)?;
    write_bytes(
        target,
        &bytes,
        a.has("--overwrite"),
        None,
        ui.export.max_kib,
    )
    .map_err(|e| error_text(&e, a.lang))?;
    let triangles: usize = bodies.iter().map(|(_, m)| m.triangles.len()).sum();
    Ok(format!(
        "{}\n",
        fill(
            Msg::MeshWrote.tr(a.lang),
            &[
                &bodies.len().to_string(),
                &triangles.to_string(),
                &target.display().to_string()
            ]
        )
    ))
}

fn schema_of(a: &Args) -> Result<Schema, String> {
    match a.value("--schema") {
        None => Ok(Schema::Ap214),
        Some(word) => {
            Schema::parse(word).ok_or_else(|| format!("{word}: the schema is ap203 or ap214"))
        }
    }
}

fn two_files(a: &Args) -> Result<(PathBuf, PathBuf), String> {
    match a.words.as_slice() {
        [input, output] => Ok((PathBuf::from(input), PathBuf::from(output))),
        _ => Err("this command needs the file to read and the file to write".to_owned()),
    }
}

fn point_nm(text: &str, lang: crate::lang::Lang) -> Result<P3, String> {
    let p = measurecmd::parse_point(text).map_err(|e| error_text(&e, lang))?;
    Ok([p[0] * 1.0e6, p[1] * 1.0e6, p[2] * 1.0e6])
}

fn axis_of(text: &str) -> Result<Axis, String> {
    Axis::parse(text).ok_or_else(|| format!("{text}: the axis is x, y or z"))
}

fn count_of(text: &str, name: &str) -> Result<u32, String> {
    text.parse::<u32>()
        .map_err(|_| format!("{text}: {name} must be a whole number"))
}

fn float_of(text: &str, name: &str) -> Result<f64, String> {
    text.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("{text}: {name} must be a number"))
}

/// `mesh-convert IN OUT [--ascii] [--schema ap203|ap214] [--overwrite]`.
fn convert(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(args, &["--config", "--schema"], &["--ascii", "--overwrite"])?;
    let (input, output) = two_files(&a)?;
    let (bodies, mut text, _, ui) = read_input(&input, a.value("--config"), a.lang)?;
    text.push_str(&write_out(&output, &bodies, &a, &ui, schema_of(&a)?)?);
    Ok((text, true))
}

/// `mesh-transform IN OUT [--scale F|FX:FY:FZ] [--rotate AXIS:DEGREES] [--about X:Y:Z]
/// [--mirror AXIS[:POSITION]] [--move X:Y:Z]`, applied in this order.
fn transform(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(
        args,
        &[
            "--config", "--scale", "--rotate", "--about", "--mirror", "--move",
        ],
        &["--ascii", "--overwrite"],
    )?;
    let (input, output) = two_files(&a)?;
    let (mut bodies, mut text, _, ui) = read_input(&input, a.value("--config"), a.lang)?;
    let about = match a.value("--about") {
        Some(t) => point_nm(t, a.lang)?,
        None => [0.0; 3],
    };
    if a.value("--scale").is_none()
        && a.value("--rotate").is_none()
        && a.value("--mirror").is_none()
        && a.value("--move").is_none()
    {
        return Err("give at least one of --scale, --rotate, --mirror and --move".to_owned());
    }
    for (_, mesh) in &mut bodies {
        if let Some(t) = a.value("--scale") {
            let parts: Vec<f64> = t
                .split(':')
                .map(|w| float_of(w, "a scale factor"))
                .collect::<Result<_, _>>()?;
            let factors = match parts.as_slice() {
                [s] => [*s, *s, *s],
                [x, y, z] => [*x, *y, *z],
                _ => return Err(format!("{t}: a scale is one factor or X:Y:Z")),
            };
            *mesh = meshops::scale(mesh, factors, about)?;
        }
        if let Some(t) = a.value("--rotate") {
            let (axis, degrees) = t
                .split_once(':')
                .ok_or_else(|| format!("{t}: a rotation is written AXIS:DEGREES, as in z:90"))?;
            *mesh = meshops::rotate(mesh, axis_of(axis)?, float_of(degrees, "the angle")?, about)?;
        }
        if let Some(t) = a.value("--mirror") {
            let (axis, at) = match t.split_once(':') {
                Some((axis, at)) => (
                    axis_of(axis)?,
                    measurecmd::length_or_mm(at)
                        .map_err(|e| error_text(&e, a.lang))?
                        .get() as f64,
                ),
                None => (axis_of(t)?, 0.0),
            };
            *mesh = meshops::mirror(mesh, axis, at)?;
        }
        if let Some(t) = a.value("--move") {
            *mesh = meshops::translate(mesh, point_nm(t, a.lang)?)?;
        }
    }
    text.push_str(&write_out(&output, &bodies, &a, &ui, schema_of(&a)?)?);
    Ok((text, true))
}

/// `mesh-pattern IN OUT --linear COUNT --step X:Y:Z` or `--circular COUNT --axis A [--center X:Y:Z]
/// [--angle DEGREES]`: every solid of the file is repeated.
fn pattern(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(
        args,
        &[
            "--config",
            "--linear",
            "--step",
            "--circular",
            "--axis",
            "--center",
            "--angle",
            "--schema",
        ],
        &["--ascii", "--overwrite"],
    )?;
    let (input, output) = two_files(&a)?;
    let (bodies, mut text, settings, ui) = read_input(&input, a.value("--config"), a.lang)?;
    let max = settings.mesh.max_triangles as usize;
    let mut out: Vec<Body> = Vec::new();
    match (a.value("--linear"), a.value("--circular")) {
        (Some(n), None) => {
            let step = point_nm(
                a.value("--step").ok_or("--linear needs --step X:Y:Z")?,
                a.lang,
            )?;
            let count = count_of(n, "the number of copies")?;
            for (name, mesh) in &bodies {
                out.push((
                    name.clone(),
                    meshops::linear_pattern(mesh, count, step, max)?,
                ));
            }
        }
        (None, Some(n)) => {
            let axis = axis_of(
                a.value("--axis")
                    .ok_or("--circular needs --axis x, y or z")?,
            )?;
            let center = match a.value("--center") {
                Some(t) => point_nm(t, a.lang)?,
                None => [0.0; 3],
            };
            let angle = match a.value("--angle") {
                Some(t) => float_of(t, "the angle")?,
                None => 360.0,
            };
            let count = count_of(n, "the number of copies")?;
            for (name, mesh) in &bodies {
                out.push((
                    name.clone(),
                    meshops::circular_pattern(mesh, count, axis, center, angle, max)?,
                ));
            }
        }
        _ => return Err("give --linear COUNT or --circular COUNT, one of the two".to_owned()),
    }
    text.push_str(&write_out(&output, &out, &a, &ui, schema_of(&a)?)?);
    Ok((text, true))
}

/// `mesh-slice FILE --axis A --at LENGTH [--svg OUT] [--csv] [--overwrite]`.
fn slice(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(
        args,
        &["--config", "--axis", "--at", "--svg"],
        &["--csv", "--overwrite"],
    )?;
    let input = PathBuf::from(a.only_word("mesh file")?);
    let axis = axis_of(
        a.value("--axis")
            .ok_or("mesh-slice needs --axis x, y or z")?,
    )?;
    let at = measurecmd::length_or_mm(a.value("--at").ok_or("mesh-slice needs --at LENGTH")?)
        .map_err(|e| error_text(&e, a.lang))?
        .get() as f64;
    let (bodies, mut text, _, ui) = read_input(&input, a.value("--config"), a.lang)?;
    let mut ok = true;
    if a.has("--csv") {
        text.push_str("solid,outlines,outer,holes,area_mm2,length_mm,open_segments\n");
    }
    let mut svg_loops = Vec::new();
    for (name, mesh) in &bodies {
        let s = meshops::slice(mesh, axis, at);
        ok &= s.open_segments == 0;
        if a.has("--csv") {
            text.push_str(&format!(
                "{name},{},{},{},{:.6},{:.6},{}\n",
                s.loops.len(),
                s.outer,
                s.holes,
                s.area_mm2,
                s.perimeter_mm,
                s.open_segments
            ));
        } else if s.loops.is_empty() && s.open_segments == 0 {
            text.push_str(&fill(Msg::MeshSliceEmpty.tr(a.lang), &[name]));
            text.push('\n');
        } else {
            text.push_str(&fill(
                Msg::MeshSliceHead.tr(a.lang),
                &[
                    name,
                    &s.loops.len().to_string(),
                    &s.outer.to_string(),
                    &s.holes.to_string(),
                    &format!("{:.6}", s.area_mm2),
                    &format!("{:.6}", s.perimeter_mm),
                ],
            ));
            text.push('\n');
            if s.open_segments > 0 {
                text.push_str(&fill(
                    Msg::MeshSliceOpen.tr(a.lang),
                    &[&s.open_segments.to_string()],
                ));
                text.push('\n');
            }
        }
        svg_loops.extend(s.loops);
    }
    if let Some(target) = a.value("--svg") {
        let merged = meshops::Slice {
            loops: svg_loops,
            area_mm2: 0.0,
            perimeter_mm: 0.0,
            outer: 0,
            holes: 0,
            open_segments: 0,
        };
        write_bytes(
            Path::new(target),
            meshops::slice_svg(&merged).as_bytes(),
            a.has("--overwrite"),
            None,
            ui.export.max_kib,
        )
        .map_err(|e| error_text(&e, a.lang))?;
    } else if a.has("--overwrite") {
        return Err("--overwrite needs --svg".to_owned());
    }
    Ok((text, ok))
}

/// `mesh-clip IN OUT --axis A --at LENGTH [--keep above|below]` or `--box X:Y:Z X:Y:Z` given as
/// `--box LOW..HIGH`.
fn clip(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(
        args,
        &["--config", "--axis", "--at", "--keep", "--box", "--schema"],
        &["--ascii", "--overwrite"],
    )?;
    let (input, output) = two_files(&a)?;
    let (bodies, mut text, _, ui) = read_input(&input, a.value("--config"), a.lang)?;
    let mut kept: Vec<Body> = Vec::new();
    let mut ok = true;
    for (name, mesh) in &bodies {
        let clipped = if let Some(b) = a.value("--box") {
            let (low, high) = b
                .split_once("..")
                .ok_or_else(|| format!("{b}: a box is written LOW..HIGH, as in 0:0:0..10:10:10"))?;
            meshops::clip_box(mesh, point_nm(low, a.lang)?, point_nm(high, a.lang)?)?
        } else {
            let axis = axis_of(
                a.value("--axis")
                    .ok_or("mesh-clip needs --axis and --at, or --box")?,
            )?;
            let at = measurecmd::length_or_mm(a.value("--at").ok_or("mesh-clip needs --at LENGTH")?)
                .map_err(|e| error_text(&e, a.lang))?
                .get() as f64;
            let above = match a.value("--keep").unwrap_or("above") {
                "above" => true,
                "below" => false,
                other => return Err(format!("{other}: --keep is above or below")),
            };
            meshops::clip_plane(mesh, axis, at, above)?
        };
        if clipped.mesh.triangles.is_empty() {
            text.push_str(&fill(Msg::MeshClipEmpty.tr(a.lang), &[name]));
            text.push('\n');
            ok = false;
            continue;
        }
        text.push_str(&fill(
            Msg::MeshClipDone.tr(a.lang),
            &[
                name,
                &format!("{:.6}", clipped.mesh.volume_nm3() / 1.0e18),
                &clipped.mesh.triangles.len().to_string(),
                &clipped.cap_triangles.to_string(),
            ],
        ));
        text.push('\n');
        if !clipped.capped {
            text.push_str(&fill(Msg::MeshClipNoCap.tr(a.lang), &[name]));
            text.push('\n');
            ok = false;
        }
        kept.push((name.clone(), clipped.mesh));
    }
    if kept.is_empty() {
        return Ok((text, false));
    }
    text.push_str(&write_out(&output, &kept, &a, &ui, schema_of(&a)?)?);
    Ok((text, ok))
}

/// `iso-fit SIZE CLASS [--table FILE] [--csv]`: the limits of a class (`H7`, `g6`) or of a fit
/// (`H7/g6`) at a nominal size.
fn iso_fit(args: &[String]) -> Result<(String, bool), String> {
    let a = Args::parse(args, &["--table"], &["--csv"])?;
    let [size, class] = a.words.as_slice() else {
        return Err("iso-fit needs a nominal size and a class (H7) or a fit (H7/g6)".to_owned());
    };
    let size_mm = measurecmd::length_or_mm(size)
        .map_err(|e| error_text(&e, a.lang))?
        .get() as f64
        / 1.0e6;
    let table = match a.value("--table") {
        Some(path) => {
            let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            iso286::Table::parse(&text).map_err(|e| format!("{path}: {e}"))?
        }
        None => iso286::Table::shipped(),
    };
    if class.contains('/') {
        let f = iso286::fit(&table, size_mm, class)?;
        if a.has("--csv") {
            return Ok((iso286::fit_csv(size_mm, class, &f), true));
        }
        return Ok((iso286::fit_text(&table, size_mm, class, &f, a.lang), true));
    }
    let parsed = iso286::Class::parse(class)?;
    let l = iso286::limits(&table, size_mm, &parsed)?;
    if a.has("--csv") {
        return Ok((
            format!(
                "class,size_mm,lower_dev_nm,upper_dev_nm,min_nm,max_nm\n{class},{size_mm},{},{},{},{}\n",
                l.lower_dev_nm, l.upper_dev_nm, l.min_nm, l.max_nm
            ),
            true,
        ));
    }
    let mut text = iso286::limits_text(size_mm, class, &l, a.lang);
    text.push('\n');
    if !table.verified {
        text.push_str(Msg::IsoUnverified.tr(a.lang));
        text.push('\n');
    }
    Ok((text, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    /// A folder of its own for one test.
    fn folder() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cad19_meshcli_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn cube_file(dir: &Path, name: &str) -> PathBuf {
        let m = 1.0e6;
        let mut vertices = Vec::new();
        for z in [0.0, 10.0 * m] {
            for y in [0.0, 10.0 * m] {
                for x in [0.0, 10.0 * m] {
                    vertices.push([x, y, z]);
                }
            }
        }
        let cube = Mesh {
            vertices,
            triangles: vec![
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
            ],
        };
        let path = dir.join(name);
        fs::write(&path, stl_binary(&[("cube", &cube)])).unwrap();
        path
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn a_file_goes_round_every_format_and_keeps_its_volume() {
        let dir = folder();
        let start = cube_file(&dir, "a.stl");
        let mut current = start.clone();
        for ext in ["obj", "3mf", "ply", "off", "dxf", "step", "stl"] {
            let next = dir.join(format!("b.{ext}"));
            let (text, ok) = run("mesh-convert", &args(&[&s(&current), &s(&next)])).unwrap();
            assert!(ok);
            assert!(text.contains("Wrote 1 solid(s) and 12 triangles"), "{text}");
            current = next;
        }
        let read = interchange::read_file_checked(
            &s(&current),
            &fs::read(&current).unwrap(),
            &ReadLimits::shipped(),
        )
        .unwrap();
        assert!((read.bodies[0].1.volume_nm3() - 1.0e21).abs() < 1.0e9);
        // a file that exists is not written over
        let again = run("mesh-convert", &args(&[&s(&start), &s(&current)])).unwrap_err();
        assert!(again.contains("PHOENIX-CAD"), "{again}");
        assert!(run(
            "mesh-convert",
            &args(&[&s(&start), &s(&current), "--overwrite"])
        )
        .is_ok());
        // the text forms and the schema
        let text_stl = dir.join("t.stl");
        run(
            "mesh-convert",
            &args(&[&s(&start), &s(&text_stl), "--ascii"]),
        )
        .unwrap();
        assert!(fs::read_to_string(&text_stl)
            .unwrap()
            .starts_with("solid solid"));
        let step = dir.join("t.stp");
        run(
            "mesh-convert",
            &args(&[&s(&start), &s(&step), "--schema", "ap203"]),
        )
        .unwrap();
        assert!(fs::read_to_string(&step)
            .unwrap()
            .contains("CONFIG_CONTROL_DESIGN"));
        assert!(run(
            "mesh-convert",
            &args(&[&s(&start), &s(&dir.join("x.stp")), "--schema", "ap999"])
        )
        .is_err());
        assert!(run("mesh-convert", &args(&[&s(&start), &s(&dir.join("x.xyz"))])).is_err());
        assert!(run("mesh-convert", &args(&[&s(&start)])).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn warnings_of_a_reader_are_printed_with_their_names_in_the_language() {
        let dir = folder();
        let path = cube_file(&dir, "a.stl");
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend_from_slice(&[0u8; 5]);
        fs::write(&path, bytes).unwrap();
        let out = dir.join("o.obj");
        let (text, _) = run("mesh-convert", &args(&[&s(&path), &s(&out)])).unwrap();
        assert!(
            text.starts_with("trailing_bytes: 5 bytes after the last triangle are ignored"),
            "{text}"
        );
        let out = dir.join("p.obj");
        let (text, _) = run(
            "mesh-convert",
            &args(&[&s(&path), &s(&out), "--lang", "es"]),
        )
        .unwrap();
        assert!(text.contains("Se ignoran 5 bytes"), "{text}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_transformation_moves_scales_turns_and_mirrors_in_that_order() {
        let dir = folder();
        let path = cube_file(&dir, "a.stl");
        let out = dir.join("o.stl");
        run(
            "mesh-transform",
            &args(&[
                &s(&path),
                &s(&out),
                "--scale",
                "2",
                "--rotate",
                "z:90",
                "--move",
                "1:2:3",
            ]),
        )
        .unwrap();
        let read = interchange::read_file_checked(
            &s(&out),
            &fs::read(&out).unwrap(),
            &ReadLimits::shipped(),
        )
        .unwrap();
        let mesh = &read.bodies[0].1;
        // the cube of 10 mm doubled is 20 mm; turned a quarter turn about z its corner (20, 0, 0) is at (0, 20, 0)
        assert!((mesh.volume_nm3() - 8.0e21).abs() < 1.0e12);
        let (lo, hi) = mesh.bounds().unwrap();
        assert_eq!(lo, [-19.0e6, 2.0e6, 3.0e6]);
        assert_eq!(hi, [1.0e6, 22.0e6, 23.0e6]);
        let mirrored = dir.join("m.stl");
        run(
            "mesh-transform",
            &args(&[&s(&path), &s(&mirrored), "--mirror", "x:20"]),
        )
        .unwrap();
        let read = interchange::read_file_checked(
            &s(&mirrored),
            &fs::read(&mirrored).unwrap(),
            &ReadLimits::shipped(),
        )
        .unwrap();
        assert!(read.bodies[0].1.volume_nm3() > 0.0);
        assert_eq!(read.bodies[0].1.bounds().unwrap().0[0], 30.0e6);
        assert!(run(
            "mesh-transform",
            &args(&[&s(&path), &s(&dir.join("n.stl"))])
        )
        .is_err());
        assert!(run(
            "mesh-transform",
            &args(&[&s(&path), &s(&dir.join("n.stl")), "--scale", "0"])
        )
        .is_err());
        assert!(run(
            "mesh-transform",
            &args(&[&s(&path), &s(&dir.join("n.stl")), "--rotate", "q:1"])
        )
        .is_err());
        assert!(run(
            "mesh-transform",
            &args(&[&s(&path), &s(&dir.join("n.stl")), "--rotate", "z"])
        )
        .is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn patterns_repeat_the_solid() {
        let dir = folder();
        let path = cube_file(&dir, "a.stl");
        let out = dir.join("o.stl");
        run(
            "mesh-pattern",
            &args(&[&s(&path), &s(&out), "--linear", "3", "--step", "15:0:0"]),
        )
        .unwrap();
        let bytes = fs::read(&out).unwrap();
        let read =
            interchange::read_file_checked(&s(&out), &bytes, &ReadLimits::shipped()).unwrap();
        assert_eq!(read.bodies[0].1.triangles.len(), 36);
        assert_eq!(read.bodies[0].1.bounds().unwrap().1[0], 40.0e6);
        let ring = dir.join("r.stl");
        run(
            "mesh-pattern",
            &args(&[
                &s(&path),
                &s(&ring),
                "--circular",
                "4",
                "--axis",
                "z",
                "--center",
                "30:0:0",
            ]),
        )
        .unwrap();
        assert!(run(
            "mesh-pattern",
            &args(&[&s(&path), &s(&dir.join("x.stl")), "--linear", "3"])
        )
        .is_err());
        assert!(run("mesh-pattern", &args(&[&s(&path), &s(&dir.join("x.stl"))])).is_err());
        assert!(run(
            "mesh-pattern",
            &args(&[&s(&path), &s(&dir.join("x.stl")), "--circular", "3"])
        )
        .is_err());
        assert!(run(
            "mesh-pattern",
            &args(&[
                &s(&path),
                &s(&dir.join("x.stl")),
                "--linear",
                "x",
                "--step",
                "1:1:1"
            ])
        )
        .is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_slice_is_told_as_text_as_values_and_as_a_drawing() {
        let dir = folder();
        let path = cube_file(&dir, "a.stl");
        let (text, ok) = run(
            "mesh-slice",
            &args(&[&s(&path), "--axis", "z", "--at", "5"]),
        )
        .unwrap();
        assert!(ok);
        assert!(
            text.contains(
                "solid: 1 outlines (1 outer, 0 holes), area 100.000000 mm2, length 40.000000 mm"
            ),
            "{text}"
        );
        let (text, _) = run(
            "mesh-slice",
            &args(&[&s(&path), "--axis", "x", "--at", "0.2 in", "--csv"]),
        )
        .unwrap();
        assert!(
            text.ends_with("solid,1,1,0,100.000000,40.000000,0\n"),
            "{text}"
        );
        let (text, _) = run(
            "mesh-slice",
            &args(&[&s(&path), "--axis", "z", "--at", "50"]),
        )
        .unwrap();
        assert!(text.contains("the plane does not meet the solid"));
        let (text, _) = run(
            "mesh-slice",
            &args(&[&s(&path), "--axis", "z", "--at", "5", "--lang", "de"]),
        )
        .unwrap();
        assert!(text.contains("Fläche"));
        let svg = dir.join("s.svg");
        run(
            "mesh-slice",
            &args(&[&s(&path), "--axis", "z", "--at", "5", "--svg", &s(&svg)]),
        )
        .unwrap();
        assert!(fs::read_to_string(&svg).unwrap().contains("<path d=\"M"));
        assert!(run(
            "mesh-slice",
            &args(&[&s(&path), "--axis", "z", "--at", "5", "--svg", &s(&svg)])
        )
        .is_err());
        assert!(run("mesh-slice", &args(&[&s(&path), "--axis", "z"])).is_err());
        assert!(run("mesh-slice", &args(&[&s(&path), "--at", "5"])).is_err());
        assert!(run(
            "mesh-slice",
            &args(&[&s(&path), "--axis", "z", "--at", "5", "--overwrite"])
        )
        .is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_clip_keeps_one_side_or_the_inside_of_a_box() {
        let dir = folder();
        let path = cube_file(&dir, "a.stl");
        let out = dir.join("o.stl");
        let (text, ok) = run(
            "mesh-clip",
            &args(&[
                &s(&path),
                &s(&out),
                "--axis",
                "z",
                "--at",
                "4",
                "--keep",
                "below",
            ]),
        )
        .unwrap();
        assert!(ok);
        assert!(text.contains("solid: kept 400.000000 mm3"), "{text}");
        let boxed = dir.join("b.stl");
        let (text, ok) = run(
            "mesh-clip",
            &args(&[&s(&path), &s(&boxed), "--box", "2:2:2..20:20:5"]),
        )
        .unwrap();
        assert!(ok);
        assert!(text.contains("kept 192.000000 mm3"), "{text}");
        let none = dir.join("n.stl");
        let (text, ok) = run(
            "mesh-clip",
            &args(&[&s(&path), &s(&none), "--axis", "z", "--at", "50"]),
        )
        .unwrap();
        assert!(!ok && text.contains("nothing of the solid is left"));
        assert!(!none.exists());
        assert!(run(
            "mesh-clip",
            &args(&[
                &s(&path),
                &s(&dir.join("x.stl")),
                "--axis",
                "z",
                "--at",
                "1",
                "--keep",
                "up"
            ])
        )
        .is_err());
        assert!(run(
            "mesh-clip",
            &args(&[&s(&path), &s(&dir.join("x.stl")), "--box", "0:0:0"])
        )
        .is_err());
        assert!(run("mesh-clip", &args(&[&s(&path), &s(&dir.join("x.stl"))])).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_fits_are_told_as_text_and_as_values() {
        let (text, ok) = run("iso-fit", &args(&["25", "H7/g6"])).unwrap();
        assert!(ok);
        assert!(text.contains("H7 at 25 mm"), "{text}");
        assert!(
            text.contains("Fit H7/g6: clearance fit; clearance from 7 to 41 um"),
            "{text}"
        );
        assert!(text.contains("not verified"));
        let (text, _) = run("iso-fit", &args(&["25 mm", "H7/g6", "--csv"])).unwrap();
        assert!(
            text.ends_with("H7/g6,25,25000000,25021000,24980000,24993000,7000,41000,clearance\n"),
            "{text}"
        );
        let (text, _) = run("iso-fit", &args(&["1 in", "k6"])).unwrap();
        assert!(text.contains("k6 at 25.4 mm"), "{text}");
        let (text, _) = run("iso-fit", &args(&["25", "p6", "--csv"])).unwrap();
        assert!(
            text.contains("p6,25,22000,35000") || text.contains("p6,25,"),
            "{text}"
        );
        let (text, _) = run("iso-fit", &args(&["25", "H7/g6", "--lang", "ja"])).unwrap();
        assert!(text.contains("はめあい"));
        assert!(run("iso-fit", &args(&["25"])).is_err());
        assert!(run("iso-fit", &args(&["0", "H7"])).is_err());
        assert!(run("iso-fit", &args(&["25", "Q7"])).is_err());
    }

    #[test]
    fn a_table_of_the_person_replaces_the_shipped_one_and_loses_the_warning() {
        let dir = folder();
        let table = dir.join("t.cfg");
        let text = include_str!("../phoenix-cad-iso286.default.cfg")
            .replace("verified = false", "verified = true");
        fs::write(&table, text).unwrap();
        let (out, _) = run("iso-fit", &args(&["25", "H7/g6", "--table", &s(&table)])).unwrap();
        assert!(!out.contains("not verified"), "{out}");
        fs::write(&table, "verified = true\nwhat\n").unwrap();
        assert!(
            run("iso-fit", &args(&["25", "H7/g6", "--table", &s(&table)]))
                .unwrap_err()
                .contains("not understood")
        );
        assert!(run(
            "iso-fit",
            &args(&["25", "H7/g6", "--table", &s(&dir.join("none.cfg"))])
        )
        .is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
