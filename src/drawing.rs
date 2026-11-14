//! Brush options and bounded per-gesture samples. Reference ink stays native-only.
use crate::{Editor, Mark, canvas::Tool, shared::SharedVec, width::WidthStop};
use eframe::egui::{self, Color32, Pos2};
pub struct Drawing {
    pub radius: f32,
    pub pressure: bool,
    pub vector_pressure: bool,
    stroke_pressure: bool,
    pub opacity: f32,
    pub hardness: f32,
    pub scroll_to_tab: bool,
    pub text: crate::typography::TextEditor,
    pub pen: crate::tablet::PenInput,
    samples: Vec<(Pos2, f32)>,
    pressure_seen: bool,
    pub reference_drag: Option<(usize, usize)>,
    pub reference_gizmo: Option<crate::mannequin::GizmoDrag>,
    pub reference_orbit: Option<(usize, Pos2, [f32; 3])>,
    pub reference_rotate: bool,
    pub reference_joint: usize,
}
impl Default for Drawing {
    fn default() -> Self {
        Self {
            radius: 0.,
            pressure: true,
            vector_pressure: false,
            stroke_pressure: true,
            opacity: 1.,
            hardness: 0.3,
            scroll_to_tab: true,
            text: Default::default(),
            pen: Default::default(),
            samples: vec![],
            pressure_seen: false,
            reference_drag: None,
            reference_gizmo: None,
            reference_orbit: None,
            reference_rotate: false,
            reference_joint: 0,
        }
    }
}
impl Drawing {
    pub fn cancel(&mut self) {
        self.samples.clear();
        self.pressure_seen = false;
        self.reference_drag = None;
        self.reference_gizmo = None;
        self.reference_orbit = None;
        self.text.editing = None;
        self.text.canvas_edit = false;
        self.text.box_drag = None;
        self.text.focus = false;
        self.text.transaction = false;
    }
    pub fn start(&mut self, p: Pos2, tool: Tool) {
        self.stroke_pressure = if tool == Tool::VectorBrush {
            self.vector_pressure
        } else {
            self.pressure
        };
        self.samples.clear();
        self.pressure_seen = false;
        self.sample(p);
    }
    pub fn sample(&mut self, p: Pos2) {
        let force = if !self.stroke_pressure {
            1.
        } else if let Some(sample) = self.pen.force() {
            let force = sample.clamp(0.02, 1.);
            if !self.pressure_seen {
                // A driver can report pen contact before its first pressure packet.
                for (_, pending) in &mut self.samples {
                    *pending = force;
                }
                self.pressure_seen = true;
            }
            force
        } else {
            // Missing packets during a pen stroke must not jump to mouse width.
            self.samples.last().map_or(1., |(_, previous)| *previous)
        };
        // Keep a bounded representation even for very long gestures.
        if self.samples.len() >= 2048 {
            self.samples = self.samples.iter().step_by(2).copied().collect();
        }
        if self
            .samples
            .last()
            .is_none_or(|(last, _)| last.distance(p) > 0.05)
        {
            self.samples.push((p, force));
        } else if let Some(last) = self.samples.last_mut() {
            last.1 = force;
        }
    }
    pub fn profile(&self, base: &[WidthStop]) -> SharedVec<WidthStop> {
        if !self.stroke_pressure || self.samples.iter().all(|(_, p)| (*p - 1.).abs() < 0.0001) {
            return base.to_vec().into();
        }
        if let [(_, pressure)] = self.samples.as_slice() {
            let width = |t| (crate::width::sample(base, t) * *pressure).clamp(0., 4.);
            return if base.is_empty() {
                vec![
                    WidthStop {
                        t: 0.,
                        width: width(0.),
                    },
                    WidthStop {
                        t: 1.,
                        width: width(1.),
                    },
                ]
                .into()
            } else {
                base.iter()
                    .map(|stop| WidthStop {
                        t: stop.t,
                        width: width(stop.t),
                    })
                    .collect()
            };
        }
        let positions: Vec<_> = self.samples.iter().map(|s| s.0).collect();
        let (lengths, total) = crate::width::lengths(&positions);
        let mut i = 0;
        (0..128)
            .map(|n| {
                let t = n as f32 / 127.;
                let distance = total * t;
                while i + 2 < lengths.len() && lengths[i + 1] < distance {
                    i += 1;
                }
                let u = ((distance - lengths[i]) / (lengths[i + 1] - lengths[i]).max(0.0001))
                    .clamp(0., 1.);
                let p = self.samples[i].1 * (1. - u) + self.samples[i + 1].1 * u;
                WidthStop {
                    t,
                    width: (crate::width::sample(base, t) * p).clamp(0., 4.),
                }
            })
            .collect()
    }
}
pub fn rounded_rectangle(a: Pos2, b: Pos2, radius: f32, style: &Mark) -> Mark {
    use crate::{
        document::Node,
        vector::{pos, xy},
    };
    let r = egui::Rect::from_two_pos(a, b);
    let radius = radius.max(0.).min(r.width().min(r.height()) * 0.5);
    let mut mark = style.clone();
    if radius < 0.001 {
        mark.rectangle = true;
        mark.nodes = Default::default();
        mark.closed = true;
        mark.points = vec![xy(a), xy(b)].into();
        return mark;
    }
    let k = radius * 0.5522848;
    let points = [
        Pos2::new(r.left() + radius, r.top()),
        Pos2::new(r.right() - radius, r.top()),
        Pos2::new(r.right(), r.top() + radius),
        Pos2::new(r.right(), r.bottom() - radius),
        Pos2::new(r.right() - radius, r.bottom()),
        Pos2::new(r.left() + radius, r.bottom()),
        Pos2::new(r.left(), r.bottom() - radius),
        Pos2::new(r.left(), r.top() + radius),
    ];
    let mut nodes: Vec<_> = points.into_iter().map(|p| Node::corner(xy(p))).collect();
    for (i, j, da, db) in [
        (1, 2, egui::vec2(k, 0.), egui::vec2(0., -k)),
        (3, 4, egui::vec2(0., k), egui::vec2(k, 0.)),
        (5, 6, egui::vec2(-k, 0.), egui::vec2(0., k)),
        (7, 0, egui::vec2(0., -k), egui::vec2(-k, 0.)),
    ] {
        nodes[i].out_handle = xy(pos(nodes[i].position) + da);
        nodes[j].in_handle = xy(pos(nodes[j].position) + db);
    }
    mark.nodes = nodes.into();
    mark.points = Default::default();
    mark.rectangle = false;
    mark.closed = true;
    mark
}
pub fn airbrush(center: &Mark, opacity: f32, hardness: f32, sketch: bool) -> Vec<Mark> {
    let points = crate::vector::flatten(center, 0.4);
    let (lengths, total) = crate::width::lengths(&points);
    if points.is_empty() {
        return vec![];
    }
    let spacing = (center.width * 0.22).max(1.5);
    let count = ((total / spacing).ceil() as usize).clamp(1, 2048);
    let mut segment = 0;
    let mut out = Vec::with_capacity(count + 1);
    for n in 0..=count {
        let t = n as f32 / count as f32;
        let distance = total * t;
        while segment + 2 < points.len() && lengths[segment + 1] < distance {
            segment += 1;
        }
        let p = if points.len() == 1 {
            points[0]
        } else {
            points[segment].lerp(
                points[segment + 1],
                ((distance - lengths[segment])
                    / (lengths[segment + 1] - lengths[segment]).max(0.001))
                .clamp(0., 1.),
            )
        };
        let radius =
            (center.width * crate::width::sample(&center.width_profile, t) * 0.5).max(0.05);
        if sketch {
            // Deterministic graphite grain: stable across previews, undo and saves.
            for grain in 0..5 {
                let seed = (n as u32)
                    .wrapping_mul(1664525)
                    .wrapping_add(grain * 1013904223);
                let x = (seed & 1023) as f32 / 511.5 - 1.;
                let y = ((seed >> 10) & 1023) as f32 / 511.5 - 1.;
                let q = p + egui::vec2(x, y) * radius;
                let mut m = Mark {
                    points: vec![
                        crate::vector::xy(q),
                        crate::vector::xy(q + egui::vec2(radius * 0.45, -radius * 0.18)),
                    ]
                    .into(),
                    width: (radius * 0.15).clamp(0.1, 100.),
                    color: [125, 128, 139, (opacity.clamp(0., 1.) * 150.) as u8],
                    ..Default::default()
                };
                m.style.guide = true;
                out.push(m);
            }
        } else {
            let mut c = center.color;
            c[3] = (c[3] as f32 * opacity.clamp(0., 1.) * 0.25).round() as u8;
            let clear = [c[0], c[1], c[2], 0];
            let mut m = crate::editing::shape(
                Tool::Ellipse,
                p - egui::Vec2::splat(radius),
                p + egui::Vec2::splat(radius),
                4,
                false,
                0.5,
                &Mark {
                    stroke: false,
                    fill: Some(c),
                    ..Default::default()
                },
            );
            m.style.gradient = Some(crate::features::Gradient {
                radial: true,
                start: crate::vector::xy(p),
                end: crate::vector::xy(p + egui::vec2(radius, 0.)),
                stops: vec![
                    crate::features::Stop { t: 0., color: c },
                    crate::features::Stop {
                        t: hardness.clamp(0.001, 0.999),
                        color: c,
                    },
                    crate::features::Stop {
                        t: 1.,
                        color: clear,
                    },
                ]
                .into(),
            });
            out.push(m);
        }
    }
    out
}
impl Editor {
    pub fn drawing_options(&mut self, ui: &mut egui::Ui) {
        if self.tool == Tool::Rectangle {
            crate::design::category(ui, "GEOMETRÍA");
            ui.add(
                egui::Slider::new(&mut self.drawing.radius, 0. ..=500.)
                    .text(crate::i18n::tr("Radio de esquina"))
                    .suffix(crate::i18n::tr(" px")),
            );
        }
        if matches!(
            self.tool,
            Tool::Brush
                | Tool::VectorBrush
                | Tool::FillBrush
                | Tool::ShadeBrush
                | Tool::LightBrush
                | Tool::Airbrush
                | Tool::SketchBrush
        ) {
            crate::design::category(ui, "TABLETA");
            let pressure = if self.tool == Tool::VectorBrush {
                &mut self.drawing.vector_pressure
            } else {
                &mut self.drawing.pressure
            };
            if crate::design::button(
                ui,
                crate::design::Icon::Pressure,
                "",
                "Activar / desactivar presión de lápiz",
                *pressure,
                true,
            )
            .clicked()
            {
                *pressure = !*pressure;
            }
            ui.label(if *pressure {
                crate::i18n::tr("Presión de lápiz activada")
            } else {
                crate::i18n::tr("Presión de lápiz desactivada")
            });
            ui.small(if !*pressure {
                crate::i18n::tr("Constant stroke width")
            } else if self.drawing.pen.detected() {
                crate::i18n::tr("Lápiz detectado · presión → grosor")
            } else {
                crate::i18n::tr("Ratón: grosor constante · esperando lápiz")
            });
        }
        if matches!(self.tool, Tool::Airbrush | Tool::SketchBrush) {
            crate::design::category(ui, "TINTA");
            ui.add(
                egui::Slider::new(&mut self.drawing.opacity, 0.01..=1.)
                    .text(crate::i18n::tr("Opacidad")),
            );
            if self.tool == Tool::Airbrush {
                ui.add(
                    egui::Slider::new(&mut self.drawing.hardness, 0. ..=1.)
                        .text(crate::i18n::tr("Dureza")),
                );
            } else {
                ui.colored_label(
                    Color32::from_rgb(170, 176, 196),
                    crate::i18n::tr("Boceto gris · no se exporta"),
                );
            }
        }
    }
}

#[cfg(test)]
mod pressure_gap_tests {
    use super::*;

    #[test]
    fn late_pressure_and_missing_release_packet_do_not_make_full_width_dots() {
        let mut drawing = Drawing::default();
        drawing.start(Pos2::ZERO, Tool::Brush);
        drawing.sample(Pos2::new(1., 0.));
        drawing.pen.test_pressure(Some(0.2));
        drawing.sample(Pos2::new(2., 0.));
        drawing.pen.test_pressure(None);
        drawing.sample(Pos2::new(3., 0.));
        let profile = drawing.profile(&[]);
        assert!(!profile.is_empty());
        assert!(
            profile
                .iter()
                .all(|sample| (sample.width - 0.2).abs() < 0.001)
        );
        drawing.start(Pos2::ZERO, Tool::Brush);
        drawing.sample(Pos2::new(1., 0.));
        assert!(drawing.profile(&[]).is_empty());
    }
}
