// SPDX-License-Identifier: GPL-3.0-or-later
//! Any closed surface of triangles as a STEP file of faceted solids (`FACETED_BREP`), in the
//! structure that CAD programs expect from the application protocols 203 (configuration controlled
//! design) and 214 (automotive design): the product, its definition and its shape, a
//! millimetre unit with its uncertainty, and one solid for each body with a face for each
//! triangle, a point for each vertex.
//!
//! The file is plain text and the same bytes come out for the same bodies. It is checked by
//! reading it back with [`crate::step_import`]; no CAD program has opened it. A mesh that is not
//! closed or has a triangle that uses a vertex twice is refused by name, because a solid that is
//! not closed cannot be a `CLOSED_SHELL`.

use std::fmt::Write;

use crate::mesh::Mesh;

/// The application protocol the file names in its header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schema {
    /// `CONFIG_CONTROL_DESIGN`, the protocol 203.
    Ap203,
    /// `AUTOMOTIVE_DESIGN`, the protocol 214.
    Ap214,
}

impl Schema {
    /// The schema by its word in the commands: `ap203` or `ap214`.
    pub fn parse(word: &str) -> Option<Schema> {
        match word {
            "ap203" => Some(Schema::Ap203),
            "ap214" => Some(Schema::Ap214),
            _ => None,
        }
    }
}

/// A coordinate in millimetres with six decimals, never `-0`.
fn real(nm: f64) -> String {
    let text = format!("{:.6}", nm / 1.0e6);
    if let Some(rest) = text.strip_prefix('-') {
        if rest.bytes().all(|b| b == b'0' || b == b'.') {
            return rest.to_owned();
        }
    }
    text
}

/// A name that fits a STEP string: ASCII letters, digits, spaces, hyphens and underscores.
fn text(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.trim().is_empty() {
        "solid".to_owned()
    } else {
        cleaned
    }
}

/// The STEP file of the bodies, refused when one of them is not a closed surface.
pub fn faceted_step(bodies: &[(&str, &Mesh)], schema: Schema) -> Result<String, String> {
    if bodies.is_empty() || bodies.iter().all(|(_, m)| m.triangles.is_empty()) {
        return Err("there is no solid to write".to_owned());
    }
    for (name, mesh) in bodies {
        mesh.check_closed()
            .map_err(|why| format!("{name}: the surface is not closed ({why})"))?;
    }
    let title = text(bodies[0].0);
    let mut out = String::from("ISO-10303-21;\nHEADER;\n");
    let _ = writeln!(
        out,
        "FILE_DESCRIPTION(('EH3D PHOENIX-CAD faceted solids'),'2;1');"
    );
    let _ = writeln!(
        out,
        "FILE_NAME('{title}.step','',(''),(''),'EH3D-PHOENIX-CAD','','');"
    );
    out.push_str(match schema {
        Schema::Ap203 => "FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));\n",
        Schema::Ap214 => "FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\n",
    });
    out.push_str("ENDSEC;\nDATA;\n");
    // the context, the unit and the product
    let (context_name, protocol, year) = match schema {
        Schema::Ap203 => (
            "configuration controlled 3D designs of mechanical parts and assemblies",
            "config_control_design",
            1994,
        ),
        Schema::Ap214 => ("automotive design", "automotive_design", 2000),
    };
    let _ = writeln!(out, "#1=APPLICATION_CONTEXT('{context_name}');");
    let _ = writeln!(
        out,
        "#2=APPLICATION_PROTOCOL_DEFINITION('international standard','{protocol}',{year},#1);"
    );
    out.push_str("#3=PRODUCT_CONTEXT('',#1,'mechanical');\n");
    let _ = writeln!(out, "#4=PRODUCT('{title}','{title}','',(#3));");
    out.push_str("#5=PRODUCT_DEFINITION_CONTEXT('part definition',#1,'design');\n");
    out.push_str("#6=PRODUCT_DEFINITION_FORMATION('','',#4);\n");
    out.push_str("#7=PRODUCT_DEFINITION('design','',#6,#5);\n");
    out.push_str("#8=PRODUCT_DEFINITION_SHAPE('','',#7);\n");
    out.push_str("#10=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\n");
    out.push_str("#11=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));\n");
    out.push_str("#12=(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT());\n");
    out.push_str("#13=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#10,'distance_accuracy_value','confusion accuracy');\n");
    out.push_str("#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#13))GLOBAL_UNIT_ASSIGNED_CONTEXT((#10,#11,#12))REPRESENTATION_CONTEXT('ID1','3D'));\n");
    out.push_str("#20=CARTESIAN_POINT('',(0.,0.,0.));\n#21=DIRECTION('',(0.,0.,1.));\n#22=DIRECTION('',(1.,0.,0.));\n");
    out.push_str("#23=AXIS2_PLACEMENT_3D('',#20,#21,#22);\n");
    let mut next = 100usize;
    let mut solids = Vec::new();
    let mut body = String::new();
    for (name, mesh) in bodies {
        let points: Vec<usize> = (0..mesh.vertices.len()).map(|k| next + k).collect();
        for (k, v) in mesh.vertices.iter().enumerate() {
            let _ = writeln!(
                body,
                "#{}=CARTESIAN_POINT('',({},{},{}));",
                points[k],
                real(v[0]),
                real(v[1]),
                real(v[2])
            );
        }
        next += mesh.vertices.len();
        let mut faces = Vec::with_capacity(mesh.triangles.len());
        for t in &mesh.triangles {
            let _ = writeln!(
                body,
                "#{}=POLY_LOOP('',(#{},#{},#{}));",
                next, points[t[0] as usize], points[t[1] as usize], points[t[2] as usize]
            );
            let _ = writeln!(body, "#{}=FACE_OUTER_BOUND('',#{},.T.);", next + 1, next);
            let _ = writeln!(body, "#{}=FACE('',(#{}));", next + 2, next + 1);
            faces.push(format!("#{}", next + 2));
            next += 3;
        }
        let _ = writeln!(body, "#{}=CLOSED_SHELL('',({}));", next, faces.join(","));
        let _ = writeln!(
            body,
            "#{}=FACETED_BREP('{}',#{});",
            next + 1,
            text(name),
            next
        );
        solids.push(format!("#{}", next + 1));
        next += 2;
    }
    out.push_str(&body);
    let _ = writeln!(
        out,
        "#{}=SHAPE_REPRESENTATION('',(#23,{}),#14);",
        next,
        solids.join(",")
    );
    let _ = writeln!(
        out,
        "#{}=SHAPE_DEFINITION_REPRESENTATION(#8,#{});",
        next + 1,
        next
    );
    out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::step_import::{read_step, StepLimits};

    fn limits() -> StepLimits {
        StepLimits::from_settings(&crate::config::Settings::defaults().mesh)
    }

    fn tetra(scale: f64) -> Mesh {
        Mesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [10.0e6 * scale, 0.0, 0.0],
                [0.0, 12.5e6 * scale, 0.0],
                [0.0, 0.0, 7.0e6 * scale],
            ],
            triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        }
    }

    #[test]
    fn two_solids_read_back_with_their_names_volumes_and_closed_surfaces() {
        let (a, b) = (tetra(1.0), tetra(2.0));
        for schema in [Schema::Ap203, Schema::Ap214] {
            let text = faceted_step(&[("small part", &a), ("big", &b)], schema).unwrap();
            let bodies = read_step(&text, &limits()).unwrap();
            assert_eq!(bodies.len(), 2);
            assert_eq!(bodies[0].0, "small part");
            assert_eq!(bodies[1].0, "big");
            assert!((bodies[0].1.volume_nm3() - a.volume_nm3()).abs() < 1.0e3);
            assert!((bodies[1].1.volume_nm3() - b.volume_nm3()).abs() < 1.0e4);
            assert!(bodies[0].1.check_closed().is_ok());
            assert_eq!(bodies[0].1.triangles.len(), 4);
        }
    }

    #[test]
    fn the_header_names_the_protocol_and_the_bytes_do_not_change() {
        let m = tetra(1.0);
        let a = faceted_step(&[("t", &m)], Schema::Ap203).unwrap();
        let b = faceted_step(&[("t", &m)], Schema::Ap214).unwrap();
        assert!(a.contains("FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));"));
        assert!(a.contains("config_control_design"));
        assert!(b.contains("AUTOMOTIVE_DESIGN"));
        assert!(a.starts_with("ISO-10303-21;\nHEADER;\n"));
        assert!(a.ends_with("ENDSEC;\nEND-ISO-10303-21;\n"));
        assert!(a.contains("SI_UNIT(.MILLI.,.METRE.)"));
        assert!(a.contains("CARTESIAN_POINT('',(0.000000,12.500000,0.000000));"));
        assert_eq!(a, faceted_step(&[("t", &m)], Schema::Ap203).unwrap());
        assert_eq!(Schema::parse("ap214"), Some(Schema::Ap214));
        assert_eq!(Schema::parse("ap242"), None);
    }

    #[test]
    fn a_surface_that_is_not_closed_and_an_empty_list_are_refused() {
        let mut open = tetra(1.0);
        open.triangles.pop();
        let e = faceted_step(&[("hole", &open)], Schema::Ap214).unwrap_err();
        assert!(e.contains("hole") && e.contains("not closed"), "{e}");
        assert!(faceted_step(&[], Schema::Ap214).is_err());
        assert!(faceted_step(&[("e", &Mesh::default())], Schema::Ap214).is_err());
    }

    #[test]
    fn names_with_quotes_and_accents_are_made_safe() {
        let m = tetra(1.0);
        let text = faceted_step(&[("it's a \u{e9}", &m)], Schema::Ap214).unwrap();
        assert!(text.contains("FACETED_BREP('it_s a _',"));
        assert!(!text.contains('\u{e9}'));
    }
}
