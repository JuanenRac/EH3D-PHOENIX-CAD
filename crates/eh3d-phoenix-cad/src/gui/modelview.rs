// SPDX-License-Identifier: GPL-3.0-or-later
//! The window that shows a file of another program: an attachment of the project of the suite,
//! the original an import kept or a file of the disk. What is read, what is told about it, the
//! order of the triangles and the corner under the pointer are decided in [`crate::modelview`];
//! this only draws.

use super::*;
use crate::modelview::{
    self, can_show, Content, Cut, Job, Look, Origin, Picture, Pose, Section, Solid, ViewOptions,
    STRAIGHT_VIEWS,
};
use crate::projection::{Camera, Point3};

/// The window, while it is open.
pub(super) struct ViewerState {
    name: String,
    /// The reading that is going on, until it answers.
    job: Option<Job>,
    content: Option<Result<Content, String>>,
    pose: Pose,
    edges: bool,
    smooth: bool,
    cut_face: bool,
    /// The section of the cut as it was last worked out, and the cut it is of.
    section: Option<(Cut, Section)>,
    measuring: bool,
    picks: Vec<Point3>,
    drawn: Option<Drawn>,
    cutting: bool,
    cut_axis: usize,
    /// Where the cut is, as a share of the way along the box of the solid.
    cut_share: f64,
}

/// The picture of a solid as it was last worked out, and what it was worked out for.
struct Drawn {
    key: (Pose, usize, usize, Look),
    picture: Picture,
    texture: egui::TextureHandle,
}

/// The picture of the pane, worked out again only when the pose, the size or the look changed.
fn picture_of<'a>(
    drawn: &'a mut Option<Drawn>,
    ctx: &egui::Context,
    solid: &Solid,
    key: (Pose, usize, usize, Look),
) -> &'a Drawn {
    if drawn.as_ref().is_some_and(|d| d.key != key) {
        *drawn = None;
    }
    drawn.get_or_insert_with(|| {
        let (pose, width, height, look) = key;
        let picture = modelview::render(solid, &pose, width, height, look);
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [picture.width, picture.height],
            &picture.rgba,
        );
        let texture = ctx.load_texture("model-view", image, egui::TextureOptions::LINEAR);
        Drawn {
            key,
            picture,
            texture,
        }
    })
}

/// The pose for a picture with `sharp` pixels to each point of the window.
fn scaled(pose: &Pose, sharp: f64) -> Pose {
    Pose {
        pan: (pose.pan.0 * sharp, pose.pan.1 * sharp),
        ..*pose
    }
}

impl App {
    fn view_options(&self) -> ViewOptions {
        ViewOptions::new(
            &self.win.settings,
            &self.win.ui.model_view,
            &self.win.sketch_settings,
        )
    }

    /// Opens the window on a file and starts reading it.
    pub(super) fn open_viewer(&mut self, origin: Origin) {
        let look = self.win.ui.model_view;
        let camera = Camera {
            yaw: self.win.ui.yaw,
            pitch: self.win.ui.pitch,
        };
        self.viewer = Some(ViewerState {
            name: origin.name(),
            job: Some(Job::start(origin, self.view_options(), self.win.lang)),
            content: None,
            pose: Pose::fitted(camera),
            edges: true,
            smooth: look.smooth,
            cut_face: look.cut_face,
            section: None,
            measuring: false,
            picks: Vec::new(),
            drawn: None,
            cutting: false,
            cut_axis: 2,
            cut_share: 0.5,
        });
    }

    /// Waits for the file that is being read, for a picture of the window.
    pub(super) fn settle_viewer(&mut self) {
        let Some(view) = &mut self.viewer else {
            return;
        };
        if let Some(job) = view.job.take() {
            view.content = Some(job.wait());
        }
    }

    /// The button over the panel of the attachments that shows the one that is chosen, when it is
    /// a 3D file or a drawing that is read here.
    pub(super) fn attachment_view_button(&mut self, ui: &mut egui::Ui) {
        let Some(item) = self
            .attachments
            .model
            .selected()
            .filter(|i| !i.is_link && can_show(&i.name))
            .cloned()
        else {
            return;
        };
        let words = fill(Msg::MvAttachment.tr(self.win.lang), &[&item.name]);
        if !ui.button(words).clicked() {
            return;
        }
        match self.attachments.model.source(&item.id) {
            Some(Ok(source)) => self.open_viewer(Origin::Attachment(Box::new(source))),
            Some(Err(why)) => self.suite_note = Some((Msg::MvOriginal, vec![why])),
            None => {}
        }
    }

    /// Shows the original that the last import kept in the file of the suite.
    pub(super) fn view_original(&mut self, file: &Path) {
        let limit = self.view_options().max_bytes as u64;
        match crate::eh3dfile::kept_original(file, limit) {
            Ok(Some((name, bytes))) => self.open_viewer(Origin::Bytes(name, bytes)),
            Ok(None) => {}
            Err(e) => {
                let why = error_text(&e, self.win.lang);
                self.suite_note = Some((Msg::MvOriginal, vec![why]));
            }
        }
    }

    /// The window of the file that is looked at, while it is open.
    pub(super) fn viewer_window(&mut self, ctx: &egui::Context) {
        let lang = self.win.lang;
        let params = self.win.params();
        let settings = self.win.ui.model_view;
        let inner = [params.inner_length, params.inner_width, params.inner_height]
            .map(|l| l.get() as f64 / 1.0e6);
        let radius = self.win.ui.measure.pick_radius;
        let wheel = self.win.ui.wheel_speed;
        let speed = self.win.ui.rotate_speed;
        let fitted = Camera {
            yaw: self.win.ui.yaw,
            pitch: self.win.ui.pitch,
        };
        let Some(view) = &mut self.viewer else {
            return;
        };
        if let Some(done) = view.job.as_ref().and_then(Job::poll) {
            view.content = Some(done);
            view.job = None;
        }
        if view.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let mut open = true;
        egui::Window::new(fill(Msg::MvTitle.tr(lang), &[&view.name]))
            .id(egui::Id::unique("model-view"))
            .open(&mut open)
            .default_pos([60.0, 60.0])
            .default_size([820.0, 600.0])
            .show(ctx, |ui| match &view.content {
                None => {
                    ui.spinner();
                    ui.label(Msg::MvLoading.tr(lang));
                }
                Some(Err(why)) => {
                    ui.colored_label(DANGER, why.as_str());
                }
                Some(Ok(content)) => {
                    for line in modelview::facts(content, Some(inner), lang) {
                        ui.label(line);
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(Msg::FitView.tr(lang)).clicked() {
                            view.pose = Pose::fitted(fitted);
                        }
                        if let Content::Solid(_) = content {
                            for (name, camera) in STRAIGHT_VIEWS {
                                if ui.button(name.tr(lang)).clicked() {
                                    view.pose = Pose::fitted(camera);
                                }
                            }
                            ui.checkbox(&mut view.edges, Msg::MvEdges.tr(lang));
                            ui.checkbox(&mut view.smooth, Msg::MvSmooth.tr(lang));
                            let measure = Msg::MeasureDistanceTool.tr(lang);
                            if ui.toggle_value(&mut view.measuring, measure).changed() {
                                view.picks.clear();
                            }
                        }
                    });
                    if view.measuring {
                        match view.picks.as_slice() {
                            [a, b] => ui.label(modelview::distance_text(*a, *b, lang)),
                            _ => ui.label(Msg::MvPickTwo.tr(lang)),
                        };
                    }
                    if let Content::Solid(solid) = content {
                        ui.horizontal_wrapped(|ui| {
                            let words = Msg::MvCut.tr(lang);
                            if ui.checkbox(&mut view.cutting, words).changed() {
                                view.picks.clear();
                            }
                            if !view.cutting {
                                return;
                            }
                            for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                                ui.radio_value(&mut view.cut_axis, axis, name);
                            }
                            let slider =
                                egui::Slider::new(&mut view.cut_share, 0.0..=1.0).show_value(false);
                            ui.add(slider);
                            let cut = Cut::of(solid, view.cut_axis, view.cut_share);
                            ui.label(format!("{:.2} mm", cut.at));
                            if !solid.closed {
                                return;
                            }
                            ui.checkbox(&mut view.cut_face, Msg::MvCutFace.tr(lang));
                            if view.section.as_ref().map(|s| s.0) != Some(cut) {
                                view.section = Some((cut, modelview::section(solid, cut)));
                            }
                            let area = view.section.as_ref().and_then(|s| s.1.area_mm2);
                            if let Some(area) = area {
                                let area = format!("{area:.2}");
                                ui.label(fill(Msg::MvCutArea.tr(lang), &[&area]));
                            }
                        });
                    }
                    ui.label(egui::RichText::new(Msg::MvHint.tr(lang)).color(DIM));
                    let size = ui.available_size().max(Vec2::new(240.0, 200.0));
                    let (response, painter) =
                        ui.allocate_painter(size, egui::Sense::click_and_drag());
                    let rect = response.rect;
                    let pane = (f64::from(rect.width()), f64::from(rect.height()));
                    let painter = painter.with_clip_rect(rect);
                    painter.rect_filled(rect, 0.0, BACKGROUND);
                    if response.dragged() {
                        let d = response.drag_delta();
                        let shift = ui.input(|i| i.modifiers.shift);
                        let moving = shift
                            || response.dragged_by(egui::PointerButton::Middle)
                            || matches!(content, Content::Flat(_));
                        if moving {
                            view.pose.pan.0 += f64::from(d.x);
                            view.pose.pan.1 += f64::from(d.y);
                        } else {
                            view.pose
                                .turn(f64::from(d.x) * speed, f64::from(d.y) * speed);
                        }
                    }
                    if response.double_clicked() && !view.measuring {
                        view.pose = Pose::fitted(fitted);
                    }
                    if response.hovered() {
                        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                        if scroll != 0.0 {
                            view.pose.zoom_by((f64::from(scroll) * wheel).exp());
                        }
                    }
                    match content {
                        Content::Solid(solid) => {
                            // one pixel of the picture for each pixel of the screen
                            let sharp = f64::from(ui.ctx().pixels_per_point()).clamp(0.5, 4.0);
                            let cut = view
                                .cutting
                                .then(|| Cut::of(solid, view.cut_axis, view.cut_share));
                            // while the view is dragged one point for each pixel is enough
                            let samples = if response.dragged() {
                                1
                            } else {
                                settings.samples as usize
                            };
                            let look = Look {
                                edges: view.edges,
                                cut,
                                smooth: view.smooth,
                                cap: view.cut_face,
                                samples,
                                threads: settings.threads as usize,
                            };
                            let key = (
                                scaled(&view.pose, sharp),
                                (pane.0 * sharp).round() as usize,
                                (pane.1 * sharp).round() as usize,
                                look,
                            );
                            let drawn = picture_of(&mut view.drawn, ui.ctx(), solid, key);
                            let whole = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                            painter.image(drawn.texture.id(), rect, whole, Color32::WHITE);
                            let clicked = response.clicked() && view.measuring;
                            if let Some(p) = response.interact_pointer_pos().filter(|_| clicked) {
                                let at = (
                                    f64::from(p.x - rect.min.x) * sharp,
                                    f64::from(p.y - rect.min.y) * sharp,
                                );
                                let found = modelview::pick(
                                    solid,
                                    &drawn.key.0,
                                    &drawn.picture,
                                    at,
                                    radius * sharp,
                                );
                                if let Some(corner) = found {
                                    if view.picks.len() >= 2 {
                                        view.picks.clear();
                                    }
                                    view.picks.push(corner);
                                }
                            }
                            let marks: Vec<Pos2> = view
                                .picks
                                .iter()
                                .map(|corner| {
                                    let (x, y) =
                                        modelview::place_of(solid, &view.pose, pane, *corner);
                                    Pos2::new(rect.min.x + x as f32, rect.min.y + y as f32)
                                })
                                .collect();
                            for mark in &marks {
                                painter.circle_filled(*mark, 4.0, MEASURE);
                            }
                            if let [a, b] = marks.as_slice() {
                                painter.line_segment([*a, *b], Stroke::new(1.8, MEASURE));
                            }
                        }
                        Content::Flat(flat) => {
                            for (a, b, round) in modelview::flat_lines(flat, &view.pose, pane) {
                                let at = |p: (f64, f64)| {
                                    Pos2::new(rect.min.x + p.0 as f32, rect.min.y + p.1 as f32)
                                };
                                let colour = if round { HOLE } else { OUTER };
                                painter.line_segment([at(a), at(b)], Stroke::new(1.3, colour));
                            }
                        }
                    }
                }
            });
        if !open {
            self.viewer = None;
        }
    }
}
