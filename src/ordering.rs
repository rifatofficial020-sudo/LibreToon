//! Draw order is the actual position in the frame, shared by rendering/export.
use crate::{
    Editor,
    canvas::Canvas,
    design::{self, Icon},
};
use eframe::egui;

#[derive(Clone, Copy)]
pub enum Order {
    Back,
    Backward,
    Forward,
    Front,
    Index(usize),
}

fn permutation(len: usize, selected: &[usize], action: Order) -> Vec<usize> {
    let mut order: Vec<_> = (0..len).collect();
    let mut mask = vec![false; len];
    for &i in selected {
        if i < len {
            mask[i] = true;
        }
    }
    match action {
        Order::Back | Order::Front | Order::Index(_) => {
            let (picked, mut rest): (Vec<_>, Vec<_>) = order.into_iter().partition(|&i| mask[i]);
            let at = match action {
                Order::Back => 0,
                Order::Front => rest.len(),
                Order::Index(i) => i.min(rest.len()),
                _ => unreachable!(),
            };
            rest.splice(at..at, picked);
            order = rest;
        }
        Order::Forward => {
            for i in (0..len.saturating_sub(1)).rev() {
                if mask[order[i]] && !mask[order[i + 1]] {
                    order.swap(i, i + 1);
                }
            }
        }
        Order::Backward => {
            for i in 1..len {
                if mask[order[i]] && !mask[order[i - 1]] {
                    order.swap(i, i - 1);
                }
            }
        }
    }
    order
}

impl Editor {
    pub fn arrange_selection(&mut self, state: &mut Canvas, action: Order) {
        if self.playing {
            return;
        }
        let partial = !state.partial.is_empty();
        if partial {
            self.checkpoint();
            self.materialize_fragments(state);
        }
        let mut layers = std::collections::BTreeMap::<usize, Vec<usize>>::new();
        for &(l, m) in &state.selected {
            if self
                .doc
                .layers
                .get(l)
                .is_some_and(|layer| !layer.options.locked && m < layer.frames[self.frame].len())
            {
                layers.entry(l).or_default().push(m);
            }
        }
        let plans: Vec<_> = layers
            .into_iter()
            .filter_map(|(l, ids)| {
                let order = permutation(self.doc.layers[l].frames[self.frame].len(), &ids, action);
                order
                    .iter()
                    .enumerate()
                    .any(|(a, &b)| a != b)
                    .then_some((l, ids, order))
            })
            .collect();
        if plans.is_empty() {
            if partial {
                self.recover();
            }
            return;
        }
        if !partial {
            self.checkpoint();
        }
        for (l, ids, order) in plans {
            let marks = &mut self.doc.layers[l].frames[self.frame];
            let before = marks.clone();
            *marks = order
                .iter()
                .map(|&i| before[i].clone())
                .collect::<Vec<_>>()
                .into();
            state.selected.retain(|&(layer, _)| layer != l);
            state.selected.extend(
                order
                    .iter()
                    .enumerate()
                    .filter(|(_, old)| ids.contains(old))
                    .map(|(i, _)| (l, i)),
            );
        }
        state.node_selection = None;
        self.style_selection.clear();
        self.recover();
    }
    pub fn order_properties_ui(&mut self, ui: &mut egui::Ui) {
        let Some(&(l, m)) = self.canvas.selected.first() else {
            return;
        };
        let max = self.doc.layers[l].frames[self.frame]
            .len()
            .saturating_sub(1);
        design::category(ui, "Orden en la capa");
        let mut action = None;
        ui.horizontal(|ui| {
            for (icon, label, op) in actions() {
                if design::button(ui, icon, "", label, false, !self.playing).clicked() {
                    action = Some(op);
                }
            }
            let mut index = m;
            if ui
                .add(egui::DragValue::new(&mut index).range(0..=max).prefix("# "))
                .on_hover_text(crate::i18n::tr("0 = fondo · números mayores = delante"))
                .changed()
            {
                action = Some(Order::Index(index));
            }
        });
        if let Some(action) = action {
            let mut state = std::mem::take(&mut self.canvas);
            self.arrange_selection(&mut state, action);
            self.canvas = state;
        }
    }
}
fn actions() -> [(Icon, &'static str, Order); 4] {
    [
        (Icon::SendBack, "Enviar al fondo", Order::Back),
        (Icon::StepBack, "Enviar atrás", Order::Backward),
        (Icon::StepFront, "Traer adelante", Order::Forward),
        (Icon::BringFront, "Traer al frente", Order::Front),
    ]
}
pub fn menu(ui: &mut egui::Ui, enabled: bool) -> Option<Order> {
    let mut result = None;
    for (icon, label, action) in actions() {
        if design::button(ui, icon, label, label, false, enabled).clicked() {
            result = Some(action);
            ui.close();
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_order_moves_selected_groups_and_preserves_other_objects() {
        assert_eq!(
            permutation(6, &[1, 2, 4], Order::Forward),
            vec![0, 3, 1, 2, 5, 4]
        );
        assert_eq!(
            permutation(6, &[1, 2, 4], Order::Backward),
            vec![1, 2, 0, 4, 3, 5]
        );
        assert_eq!(
            permutation(6, &[1, 4], Order::Front),
            vec![0, 2, 3, 5, 1, 4]
        );
        assert_eq!(permutation(6, &[1, 4], Order::Back), vec![1, 4, 0, 2, 3, 5]);
        assert_eq!(
            permutation(6, &[1, 4], Order::Index(2)),
            vec![0, 2, 1, 4, 3, 5]
        );
    }
}
