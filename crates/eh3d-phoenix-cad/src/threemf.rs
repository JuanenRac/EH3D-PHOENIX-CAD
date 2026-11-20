// SPDX-License-Identifier: GPL-3.0-or-later
//! The reader of 3MF packages written by other programs: the model is found through the
//! relationships of the package, the elements are read whatever the quotes, spaces and prefixes
//! of the XML, the objects made of components of other objects are put together, and the
//! transformation of each item of the build is applied (a mirror turns the triangles the other way
//! round so that the solid stays outside in).
//!
//! The reader never panics on a damaged model and stops at the limits it is given. Its refusals
//! say what is wrong: an element that is not closed, a unit that is not known, a vertex that does
//! not exist, an object that is not there, components that nest too deep and components that live
//! in other files of the package (not read).

use std::collections::BTreeMap;

use crate::interchange::Body;
use crate::mesh::Mesh;
use crate::zipread::{read_zip, ZipLimits};

/// The deepest nesting of components.
const MAX_DEPTH: usize = 16;

/// The farthest a vertex may be from the origin, in nanometres (a thousand kilometres).
const FAR_NM: f64 = 1.0e15;

/// A piece of the XML: an opening element with its attributes (empty when it closes itself) or a
/// closing one. Names have no prefix.
pub(crate) enum Token {
    Open {
        name: String,
        attributes: Vec<(String, String)>,
        empty: bool,
    },
    Close(String),
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_owned()
}

fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    text.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// The elements of an XML text, without the comments, the declarations, the text between them or
/// any entity but the five of the language.
pub(crate) fn tokens(xml: &str) -> Result<Vec<Token>, String> {
    let bytes = xml.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    let bad = |why: &str| format!("the model is not well formed: {why}");
    while at < bytes.len() {
        let Some(open) = xml[at..].find('<') else {
            break;
        };
        at += open;
        let rest = &xml[at..];
        if rest.starts_with("<!--") {
            let end = rest
                .find("-->")
                .ok_or_else(|| bad("a comment is not closed"))?;
            at += end + 3;
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            let end = rest
                .find("]]>")
                .ok_or_else(|| bad("a CDATA is not closed"))?;
            at += end + 3;
            continue;
        }
        if rest.starts_with("<?") {
            let end = rest
                .find("?>")
                .ok_or_else(|| bad("a declaration is not closed"))?;
            at += end + 2;
            continue;
        }
        if rest.starts_with("<!") {
            let end = rest
                .find('>')
                .ok_or_else(|| bad("a declaration is not closed"))?;
            at += end + 1;
            continue;
        }
        let mut i = at + 1;
        let closing = bytes.get(i) == Some(&b'/');
        if closing {
            i += 1;
        }
        let name_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && !matches!(bytes[i], b'/' | b'>')
        {
            i += 1;
        }
        let name = local(&xml[name_start..i]);
        if name.is_empty() {
            return Err(bad("an element has no name"));
        }
        let mut attributes = Vec::new();
        let mut empty = false;
        loop {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            match bytes.get(i) {
                None => return Err(bad("an element is not finished")),
                Some(b'>') => {
                    i += 1;
                    break;
                }
                Some(b'/') => {
                    empty = true;
                    i += 1;
                }
                Some(_) => {
                    let key_start = i;
                    while i < bytes.len()
                        && !bytes[i].is_ascii_whitespace()
                        && !matches!(bytes[i], b'=' | b'>' | b'/')
                    {
                        i += 1;
                    }
                    let key = local(&xml[key_start..i]);
                    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if bytes.get(i) != Some(&b'=') {
                        return Err(bad("an attribute has no value"));
                    }
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    let quote = match bytes.get(i) {
                        Some(q @ (b'"' | b'\'')) => *q,
                        _ => return Err(bad("a value is not quoted")),
                    };
                    i += 1;
                    let value_start = i;
                    while i < bytes.len() && bytes[i] != quote {
                        i += 1;
                    }
                    if i >= bytes.len() {
                        return Err(bad("a value is not closed"));
                    }
                    attributes.push((key, unescape(&xml[value_start..i])));
                    i += 1;
                }
            }
        }
        at = i;
        if closing {
            out.push(Token::Close(name));
        } else {
            out.push(Token::Open {
                name,
                attributes,
                empty,
            });
        }
    }
    Ok(out)
}

pub(crate) fn attr<'a>(attributes: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

/// A transformation of the 3MF file: twelve numbers, the rows of a matrix with the translation in
/// the last one, applied to a point written as a row.
type Transform = [f64; 12];

const IDENTITY: Transform = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];

fn parse_transform(text: &str) -> Result<Transform, String> {
    let numbers: Vec<f64> = text
        .split_whitespace()
        .map(|w| w.parse::<f64>().ok().filter(|v| v.is_finite()))
        .collect::<Option<Vec<f64>>>()
        .ok_or_else(|| format!("{text}: a transformation holds numbers"))?;
    numbers
        .try_into()
        .map_err(|_| format!("{text}: a transformation holds twelve numbers"))
}

/// `outer` applied after `inner`.
fn compose(inner: &Transform, outer: &Transform) -> Transform {
    let mut out = [0.0; 12];
    for row in 0..4 {
        for col in 0..3 {
            let a = |k: usize| {
                if row < 3 {
                    inner[row * 3 + k]
                } else {
                    inner[9 + k]
                }
            };
            let mut sum = a(0) * outer[col] + a(1) * outer[3 + col] + a(2) * outer[6 + col];
            if row == 3 {
                sum += outer[9 + col];
            }
            out[row * 3 + col] = sum;
        }
    }
    out
}

fn determinant(m: &Transform) -> f64 {
    m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
        + m[2] * (m[3] * m[7] - m[4] * m[6])
}

fn apply(m: &Transform, p: [f64; 3]) -> [f64; 3] {
    [
        p[0] * m[0] + p[1] * m[3] + p[2] * m[6] + m[9],
        p[0] * m[1] + p[1] * m[4] + p[2] * m[7] + m[10],
        p[0] * m[2] + p[1] * m[5] + p[2] * m[8] + m[11],
    ]
}

#[derive(Default)]
struct Object {
    name: String,
    skip: bool,
    vertices: Vec<[f64; 3]>,
    triangles: Vec<[u32; 3]>,
    components: Vec<(u64, Transform)>,
    remote: bool,
}

struct Model {
    scale: f64,
    objects: BTreeMap<u64, Object>,
    order: Vec<u64>,
    build: Vec<(u64, Transform)>,
}

fn id_of(attributes: &[(String, String)], key: &str, what: &str) -> Result<u64, String> {
    attr(attributes, key)
        .and_then(|t| t.trim().parse::<u64>().ok())
        .ok_or_else(|| format!("{what} has no valid {key}"))
}

fn unit_nanometres(unit: &str) -> Option<f64> {
    match unit {
        "micron" => Some(1_000.0),
        "millimeter" => Some(1.0e6),
        "centimeter" => Some(1.0e7),
        "inch" => Some(25.4e6),
        "foot" => Some(304.8e6),
        "meter" => Some(1.0e9),
        _ => None,
    }
}

fn parse_model(xml: &str, max_triangles: usize) -> Result<Model, String> {
    let mut model = Model {
        scale: 1.0e6,
        objects: BTreeMap::new(),
        order: Vec::new(),
        build: Vec::new(),
    };
    let mut stack: Vec<String> = Vec::new();
    let mut current: Option<u64> = None;
    let mut triangles = 0usize;
    let mut saw_model = false;
    for token in tokens(xml)? {
        match token {
            Token::Open {
                name,
                attributes,
                empty,
            } => {
                match name.as_str() {
                    "model" => {
                        saw_model = true;
                        let unit = attr(&attributes, "unit").unwrap_or("millimeter");
                        model.scale = unit_nanometres(unit)
                            .ok_or_else(|| format!("the unit {unit} is not known"))?;
                    }
                    "object" => {
                        let id = id_of(&attributes, "id", "an object")?;
                        let kind = attr(&attributes, "type").unwrap_or("model");
                        let object = Object {
                            name: attr(&attributes, "name").unwrap_or("").to_owned(),
                            skip: matches!(kind, "support" | "solidsupport" | "surface"),
                            ..Object::default()
                        };
                        if model.objects.insert(id, object).is_some() {
                            return Err(format!("the object {id} is listed twice"));
                        }
                        model.order.push(id);
                        if !empty {
                            current = Some(id);
                        }
                    }
                    "vertex" => {
                        let id = current.ok_or("a vertex is outside an object")?;
                        if let Some(object) = model.objects.get_mut(&id) {
                            let mut p = [0.0; 3];
                            for (slot, key) in p.iter_mut().zip(["x", "y", "z"]) {
                                *slot = attr(&attributes, key)
                                    .and_then(|t| t.trim().parse::<f64>().ok())
                                    .filter(|v| v.is_finite())
                                    .ok_or_else(|| {
                                        format!("a vertex of {} has no valid {key}", object.name)
                                    })?;
                            }
                            object.vertices.push(p);
                        }
                    }
                    "triangle" => {
                        let id = current.ok_or("a triangle is outside an object")?;
                        triangles += 1;
                        if triangles > max_triangles {
                            return Err(format!(
                                "the model holds more than {max_triangles} triangles"
                            ));
                        }
                        if let Some(object) = model.objects.get_mut(&id) {
                            let count = object.vertices.len();
                            let mut t = [0u32; 3];
                            for (slot, key) in t.iter_mut().zip(["v1", "v2", "v3"]) {
                                *slot = attr(&attributes, key)
                                    .and_then(|v| v.trim().parse::<u32>().ok())
                                    .filter(|v| (*v as usize) < count)
                                    .ok_or_else(|| {
                                        format!("a triangle of {} has no vertex {key}", object.name)
                                    })?;
                            }
                            object.triangles.push(t);
                        }
                    }
                    "component" => {
                        let id = current.ok_or("a component is outside an object")?;
                        let target = id_of(&attributes, "objectid", "a component")?;
                        let transform = match attr(&attributes, "transform") {
                            Some(text) => parse_transform(text)?,
                            None => IDENTITY,
                        };
                        let remote = attr(&attributes, "path").is_some();
                        if let Some(object) = model.objects.get_mut(&id) {
                            object.remote |= remote;
                            object.components.push((target, transform));
                        }
                    }
                    "item" => {
                        let target = id_of(&attributes, "objectid", "an item of the build")?;
                        let transform = match attr(&attributes, "transform") {
                            Some(text) => parse_transform(text)?,
                            None => IDENTITY,
                        };
                        model.build.push((target, transform));
                    }
                    _ => {}
                }
                if !empty {
                    stack.push(name);
                }
            }
            Token::Close(name) => {
                if stack.pop().as_deref() != Some(name.as_str()) {
                    return Err(format!(
                        "the model is not well formed: </{name}> closes nothing"
                    ));
                }
                if name == "object" {
                    current = None;
                }
            }
        }
    }
    if let Some(open) = stack.last() {
        return Err(format!(
            "the model is not well formed: <{open}> is not closed"
        ));
    }
    if !saw_model {
        return Err("the model has no <model> element".to_owned());
    }
    Ok(model)
}

/// The vertices and triangles of an object and of its components, put in the place the
/// transformation says.
fn flatten(
    model: &Model,
    id: u64,
    transform: &Transform,
    depth: usize,
    mesh: &mut Mesh,
    budget: &mut usize,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!(
            "the components nest deeper than {MAX_DEPTH} levels (or call themselves)"
        ));
    }
    let object = model
        .objects
        .get(&id)
        .ok_or_else(|| format!("the object {id} is not in the model"))?;
    if object.remote {
        return Err(format!(
            "the object {id} has components in other files of the package, which are not read"
        ));
    }
    let flip = determinant(transform) < 0.0;
    let first = mesh.vertices.len() as u32;
    for v in &object.vertices {
        let p = apply(transform, *v);
        let nm = [
            (p[0] * model.scale).round(),
            (p[1] * model.scale).round(),
            (p[2] * model.scale).round(),
        ];
        if nm.iter().any(|c| !c.is_finite() || c.abs() > FAR_NM) {
            return Err(format!(
                "a vertex of {} is too far from the origin",
                object.name
            ));
        }
        mesh.vertices.push(nm);
    }
    *budget = budget
        .checked_sub(object.triangles.len())
        .ok_or("the solids put together hold more triangles than the model")?;
    for t in &object.triangles {
        let (a, b, c) = (first + t[0], first + t[1], first + t[2]);
        mesh.triangles
            .push(if flip { [a, c, b] } else { [a, b, c] });
    }
    for (child, local) in &object.components {
        flatten(
            model,
            *child,
            &compose(local, transform),
            depth + 1,
            mesh,
            budget,
        )?;
    }
    Ok(())
}

/// Where the model of a package is: the target of the relationship of the 3D model, or the usual
/// place.
fn model_path(entries: &[(String, Vec<u8>)]) -> Result<String, String> {
    let usual = "3D/3dmodel.model".to_owned();
    let Some((_, rels)) = entries.iter().find(|(n, _)| n == "_rels/.rels") else {
        return Ok(usual);
    };
    let Ok(text) = std::str::from_utf8(rels) else {
        return Ok(usual);
    };
    for token in tokens(text).unwrap_or_default() {
        if let Token::Open {
            name, attributes, ..
        } = token
        {
            if name == "Relationship"
                && attr(&attributes, "Type").is_some_and(|t| t.ends_with("3dmodel"))
            {
                if let Some(target) = attr(&attributes, "Target") {
                    let path = target.trim_start_matches('/').to_owned();
                    if entries.iter().any(|(n, _)| *n == path) {
                        return Ok(path);
                    }
                    return Err(format!(
                        "the package has no {path}, which its relationships name"
                    ));
                }
            }
        }
    }
    Ok(usual)
}

/// The solids of a 3MF package: one for each item of the build (or for each object when the build
/// is empty), with the transformation of the item applied and the components put together.
pub fn read_package(
    bytes: &[u8],
    zip: &ZipLimits,
    max_triangles: usize,
) -> Result<Vec<Body>, String> {
    let entries = read_zip(bytes, zip)?;
    let path = model_path(&entries)?;
    let model_bytes = entries
        .iter()
        .find(|(n, _)| *n == path)
        .ok_or_else(|| format!("the package has no {path}"))?;
    let xml =
        std::str::from_utf8(&model_bytes.1).map_err(|_| "the model is not text".to_owned())?;
    let model = parse_model(xml, max_triangles)?;
    let items: Vec<(u64, Transform)> = if model.build.is_empty() {
        model
            .order
            .iter()
            .filter(|id| !model.objects[*id].skip)
            .map(|id| (*id, IDENTITY))
            .collect()
    } else {
        model.build.clone()
    };
    let mut budget = max_triangles;
    let mut bodies = Vec::new();
    for (id, transform) in items {
        let mut mesh = Mesh::default();
        flatten(&model, id, &transform, 0, &mut mesh, &mut budget)?;
        let name = model
            .objects
            .get(&id)
            .map(|o| o.name.clone())
            .unwrap_or_default();
        bodies.push((name, mesh));
    }
    Ok(bodies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zipread::write_zip_deflate;

    fn limits() -> ZipLimits {
        ZipLimits::new(100, 4096)
    }

    /// The model of a tetrahedron as object `id`, with spaces, tabs and single quotes as another
    /// program writes them.
    fn tetra(id: u32) -> String {
        format!(
            "<object id='{id}' name='tetra {id}' type='model'>\n\t<mesh>\n\t<vertices>\n\
             <vertex x='0' y='0' z='0'/><vertex x='10' y = '0' z='0' />\n\
             <vertex x='0' y='10'\n z='0'/><vertex x='0' y='0' z='10'/>\n</vertices>\n\
             <triangles><triangle v1='0' v2='2' v3='1'/><triangle v1='0' v2='1' v3='3'/>\
             <triangle v1='0' v2='3' v3='2'/><triangle v1='1' v2='2' v3='3'/></triangles>\n\
             </mesh></object>"
        )
    }

    fn package(model: &str) -> Vec<u8> {
        write_zip_deflate(&[("3D/3dmodel.model", model.as_bytes())])
    }

    fn wrap(unit: &str, resources: &str, build: &str) -> String {
        format!(
            "<?xml version='1.0'?><!-- made elsewhere -->\n<m:model unit='{unit}' \
             xmlns:m='http://schemas.microsoft.com/3dmanufacturing/core/2015/02'>\
             <m:resources>{resources}</m:resources><m:build>{build}</m:build></m:model>"
        )
    }

    #[test]
    fn a_deflated_package_with_single_quotes_and_prefixes_is_read() {
        let model = wrap("millimeter", &tetra(1), "<item objectid='1'/>")
            .replace("<m:", "<")
            .replace("</m:", "</");
        let bodies = read_package(&package(&model), &limits(), 1000).unwrap();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].0, "tetra 1");
        assert_eq!(bodies[0].1.triangles.len(), 4);
        assert!((bodies[0].1.volume_nm3() - 1.0e3 / 6.0 * 1.0e18).abs() < 1.0e12);
    }

    #[test]
    fn the_prefix_of_the_namespace_is_ignored() {
        let model = wrap("millimeter", &tetra(1), "<item objectid='1'/>");
        assert!(model.contains("<m:model"));
        let bodies = read_package(&package(&model), &limits(), 1000).unwrap();
        assert_eq!(bodies[0].1.vertices.len(), 4);
    }

    #[test]
    fn the_transformation_of_an_item_moves_turns_and_scales_in_the_unit_of_the_file() {
        // a quarter turn about z, then a move of 2 cm along x; the unit is the centimetre
        let build = "<item objectid='1' transform='0 1 0 -1 0 0 0 0 1 2 0 0'/>";
        let model = wrap("centimeter", &tetra(1), build);
        let bodies = read_package(&package(&model), &limits(), 1000).unwrap();
        let mesh = &bodies[0].1;
        // the point (10, 0, 0) cm goes to (2, 10, 0) cm
        assert!(mesh.vertices.contains(&[2.0e7, 1.0e8, 0.0]));
        // a pure turn keeps the volume and the way the surface faces
        assert!(mesh.volume_nm3() > 0.0);
    }

    #[test]
    fn a_mirror_turns_the_triangles_so_that_the_solid_stays_outside_in() {
        let build = "<item objectid='1' transform='-1 0 0 0 1 0 0 0 1 0 0 0'/>";
        let model = wrap("millimeter", &tetra(1), build);
        let mirrored = read_package(&package(&model), &limits(), 1000).unwrap();
        let plain = read_package(
            &package(&wrap("millimeter", &tetra(1), "<item objectid='1'/>")),
            &limits(),
            1000,
        )
        .unwrap();
        assert!(mirrored[0].1.volume_nm3() > 0.0);
        assert!((mirrored[0].1.volume_nm3() - plain[0].1.volume_nm3()).abs() < 1.0);
    }

    #[test]
    fn an_object_of_components_is_put_together_and_each_item_is_a_solid() {
        let assembly = "<object id='9' name='pair'><components>\
            <component objectid='1'/>\
            <component objectid='2' transform='1 0 0 0 1 0 0 0 1 20 0 0'/>\
            </components></object>";
        let model = wrap(
            "millimeter",
            &format!("{}{}{}", tetra(1), tetra(2), assembly),
            "<item objectid='9'/><item objectid='1' transform='1 0 0 0 1 0 0 0 1 0 0 50'/>",
        );
        let bodies = read_package(&package(&model), &limits(), 1000).unwrap();
        assert_eq!(bodies.len(), 2);
        assert_eq!(bodies[0].0, "pair");
        assert_eq!(bodies[0].1.vertices.len(), 8);
        assert_eq!(bodies[0].1.triangles.len(), 8);
        assert!(bodies[0].1.vertices.contains(&[3.0e7, 0.0, 0.0]));
        assert!(bodies[1].1.vertices.contains(&[0.0, 0.0, 6.0e7]));
    }

    #[test]
    fn without_a_build_every_object_is_a_solid_and_the_supports_are_left_out() {
        let support = tetra(2).replace("type='model'", "type='support'");
        let model = wrap("millimeter", &format!("{}{}", tetra(1), support), "");
        let bodies = read_package(&package(&model), &limits(), 1000).unwrap();
        assert_eq!(bodies.len(), 1);
    }

    #[test]
    fn refusals_say_what_is_wrong() {
        let ok = wrap("millimeter", &tetra(1), "<item objectid='1'/>");
        let read = |m: &str| read_package(&package(m), &limits(), 1000).unwrap_err();
        assert!(read(&ok.replace("millimeter", "furlong")).contains("furlong"));
        assert!(read(&ok.replace("v3='1'", "v3='99'")).contains("v3"));
        assert!(read(&ok.replace("</object>", "")).contains("not well formed"));
        assert!(read(&ok.replace("objectid='1'", "objectid='7'")).contains("object 7"));
        assert!(read(&ok.replace("x='10'", "x='nan'")).contains("no valid x"));
        assert!(read(&ok.replace("x='10'", "x='1e300'")).contains("too far"));
        assert!(read(&ok.replace(
            "<item objectid='1'/>",
            "<item objectid='1' transform='1 2 3'/>"
        ))
        .contains("twelve"));
        assert!(read(&ok.replace("name='tetra 1'", "name=tetra")).contains("not quoted"));
        let cycle = wrap(
            "millimeter",
            "<object id='1'><components><component objectid='1'/></components></object>",
            "<item objectid='1'/>",
        );
        assert!(read(&cycle).contains("deeper"));
        let remote = wrap(
            "millimeter",
            "<object id='1'><components><component objectid='2' p:path='/3D/other.model'/></components></object>",
            "<item objectid='1'/>",
        );
        assert!(read(&remote).contains("other files"));
        assert!(read_package(&package(&ok), &limits(), 3)
            .unwrap_err()
            .contains("more than 3"));
    }

    #[test]
    fn the_relationships_name_the_model_when_it_is_not_in_the_usual_place() {
        let model = wrap("millimeter", &tetra(1), "<item objectid='1'/>");
        let rels = "<Relationships><Relationship Target='/3D/part.model' Id='r' \
                    Type='http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel'/></Relationships>";
        let zip = write_zip_deflate(&[
            ("_rels/.rels", rels.as_bytes()),
            ("3D/part.model", model.as_bytes()),
        ]);
        assert_eq!(read_package(&zip, &limits(), 1000).unwrap().len(), 1);
        let zip = write_zip_deflate(&[("_rels/.rels", rels.as_bytes())]);
        assert!(read_package(&zip, &limits(), 1000)
            .unwrap_err()
            .contains("3D/part.model"));
    }

    #[test]
    fn damaged_models_never_panic_with_a_fixed_seed() {
        let model = wrap(
            "millimeter",
            &format!("{}<object id='5'><components><component objectid='1' transform='1 0 0 0 1 0 0 0 1 1 2 3'/></components></object>", tetra(1)),
            "<item objectid='5'/>",
        );
        let mut seed = 99u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as usize
        };
        let base = model.into_bytes();
        for _ in 0..1500 {
            let mut bytes = base.clone();
            for _ in 0..1 + next() % 5 {
                let at = next() % bytes.len();
                bytes[at] = b" <>/='\"x9&-"[next() % 11];
            }
            let cut = next() % (bytes.len() + 1);
            if let Ok(text) = String::from_utf8(bytes[..cut].to_vec()) {
                let _ = parse_model(&text, 100);
            }
        }
    }
}
