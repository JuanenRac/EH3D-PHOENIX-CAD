// SPDX-License-Identifier: GPL-3.0-or-later
//! The 2D drawing sheets of the design: views of the solids of the history, dimensions that follow
//! the model, layers, balloons tied to the bill of materials, and the files that come out of them.
//!
//! * [`settings`]: the sizes of the sheets (ISO A4 to A0 and any format the person adds), the style
//!   of the dimensions, the title block, the layers and the limits, all from the section `[sheet]`
//!   of a settings file;
//! * [`model`]: the sheet (format, orientation, scale, projection), its views, dimensions and
//!   balloons, and the optional domain `drawing` of the project file: one canonical text with a
//!   version and the lines it does not know kept as they are;
//! * [`geom`]: the edges of a solid of aligned prisms projected on the front, top, right and
//!   isometric views, visible or hidden;
//! * [`measure`]: dimensions tied to the nodes of the history and the elements of the sketches by
//!   their ids, worked out again from the model every time, with the orphans found and named;
//! * [`bom`]: the bill of materials of the history and the stable numbers of its items;
//! * [`sectioning`]: the section views (the cut, the hatching of the cut faces, the line of the cut)
//!   and the detail views (the part of a view inside a circle or a rectangle, enlarged);
//! * [`tol`]: the tolerances of a dimension, classes of ISO 286 (`H7`, `H7/g6`) or deviations;
//! * [`extended`]: chain and ordinate dimensions, the placing of dimension lines so that they do
//!   not lie on one another, and the dragging of a dimension line;
//! * [`scene`]: the whole sheet as primitives on layers, with the problems found;
//! * [`out`]: the SVG, PDF and DXF files of a sheet;
//! * [`panel`]: the state and the lines of the panel of the window, with no window in it.
//!
//! Nothing here has been opened in a drawing program, and the views have been tried on solids made
//! by the tests only.

pub mod bom;
pub mod extended;
pub mod geom;
pub mod measure;
pub mod model;
pub mod out;
pub mod panel;
pub mod prim;
pub mod scene;
pub mod sectioning;
pub mod settings;
pub mod tol;

#[cfg(test)]
mod tests;

pub use measure::{Issue, Model};
pub use model::{
    Axis, Balloon, Detail, DetailShape, Dim, DimKind, Place, Plan, Ref, Revision, SectionCut,
    SheetScale, Side, Source, Tol, View, ViewKind, DRAWING_DOMAIN,
};
pub use scene::{evaluate, Evaluation};
pub use settings::SheetSettings;
