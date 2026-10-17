//! Widget di supporto per i form (righe etichetta + campo dentro una Grid).

use eframe::egui::{self, RichText, TextEdit, Ui};

pub fn grid(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id).num_columns(2).spacing([14.0, 8.0]).min_col_width(190.0).show(ui, add);
}

pub fn section(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).strong().size(16.0));
    ui.separator();
    add(ui);
}

pub fn text(ui: &mut Ui, label: &str, v: &mut String, hint: &str) {
    ui.label(label);
    ui.add(TextEdit::singleline(v).hint_text(hint).desired_width(f32::INFINITY));
    ui.end_row();
}

pub fn secret(ui: &mut Ui, label: &str, v: &mut String, hint: &str) {
    ui.label(label);
    ui.add(TextEdit::singleline(v).password(true).hint_text(hint).desired_width(f32::INFINITY));
    ui.end_row();
}

pub fn check(ui: &mut Ui, label: &str, v: &mut bool, text: &str) {
    ui.label(label);
    ui.checkbox(v, text);
    ui.end_row();
}

pub fn combo(ui: &mut Ui, label: &str, idx: &mut usize, items: &[&str]) {
    ui.label(label);
    egui::ComboBox::from_id_salt(label)
        .selected_text(items[(*idx).min(items.len() - 1)])
        .width(420.0)
        .show_ui(ui, |ui| {
            for (i, t) in items.iter().enumerate() {
                ui.selectable_value(idx, i, *t);
            }
        });
    ui.end_row();
}

#[derive(Clone, Copy)]
pub enum Pick {
    Dir,
    File,
}

pub fn path(ui: &mut Ui, label: &str, v: &mut String, hint: &str, kind: Pick) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(TextEdit::singleline(v).hint_text(hint).desired_width((ui.available_width() - 100.0).max(100.0)));
        if ui.button("Sfoglia…").clicked() {
            let d = rfd::FileDialog::new();
            let p = match kind {
                Pick::Dir => d.pick_folder(),
                Pick::File => d.pick_file(),
            };
            if let Some(p) = p {
                *v = p.to_string_lossy().trim_start_matches(r"\\?\").to_string();
            }
        }
    });
    ui.end_row();
}

pub fn note(ui: &mut Ui, t: &str) {
    ui.label(RichText::new(t).weak().small());
}

/// Legge un numero dentro un intervallo, con messaggio d'errore per l'utente.
pub fn num<T: std::str::FromStr + PartialOrd + std::fmt::Display + Copy>(
    label: &str,
    s: &str,
    min: T,
    max: T,
) -> Result<T, String> {
    match s.trim().parse::<T>() {
        Ok(v) if v >= min && v <= max => Ok(v),
        _ => Err(format!("{label}: inserisci un numero tra {min} e {max}")),
    }
}

pub fn req(label: &str, s: &str) -> Result<(), String> {
    if s.trim().is_empty() {
        Err(format!("{label}: campo obbligatorio"))
    } else {
        Ok(())
    }
}
