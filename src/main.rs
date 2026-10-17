//! SSAS AI — installazione guidata (interfaccia grafica).
//!
//! Raccoglie tutti i parametri in un form, scrive i file necessari (.env,
//! docker-compose, manifest Kubernetes, config.json del proxy) ed esegue
//! l'installazione mostrando l'avanzamento in tempo reale.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod common;
mod docker;
mod gui;
mod k8s;
mod proxy;
mod ui;
mod update;
mod winapp;

use common::*;
use eframe::egui::{self, Color32, RichText};
use gui::*;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use ui::{Lvl, Msg};

const MODES: [(&str, &str); 5] = [
    ("Docker", "docker compose: app + PostgreSQL"),
    ("Kubernetes", "manifest, ingress/HTTPS, autoscaling"),
    ("Windows", "app nativa come servizio"),
    ("Proxy SSAS", "ponte REST sul server SSAS"),
    ("Aggiorna", "aggiorna un'installazione esistente"),
];

struct App {
    mode: usize,
    root_text: String,
    out_text: String,
    dry_run: bool,
    admin: bool,
    docker: docker::DockerForm,
    k8s: k8s::K8sForm,
    win: winapp::WinForm,
    proxy: proxy::ProxyForm,
    update: update::UpdateForm,
    show_log: bool,
    log: Vec<(Lvl, String)>,
    rx: Option<Receiver<Msg>>,
    running: bool,
    status: Option<(bool, String)>,
    error: Option<String>,
}

impl App {
    fn new() -> Self {
        let root = find_root();
        let out = std::env::current_dir().unwrap_or_default().join("ssas-ai-install");
        Self {
            mode: 0,
            root_text: root.as_ref().map(|r| clean_path(r)).unwrap_or_default(),
            out_text: out.to_string_lossy().into_owned(),
            dry_run: false,
            admin: is_admin(),
            docker: docker::DockerForm::new(root.is_some()),
            k8s: k8s::K8sForm::new(),
            win: winapp::WinForm::new(),
            proxy: proxy::ProxyForm::new(),
            update: update::UpdateForm::new(&out),
            show_log: false,
            log: vec![],
            rx: None,
            running: false,
            status: None,
            error: None,
        }
    }

    fn root(&self) -> Option<PathBuf> {
        let p = PathBuf::from(self.root_text.trim().trim_matches('"'));
        is_project_root(&p).then(|| p.canonicalize().unwrap_or(p))
    }

    fn ctx(&self) -> Ctx {
        Ctx { root: self.root(), dry_run: self.dry_run, out_base: PathBuf::from(self.out_text.trim()) }
    }

    fn needs_admin(&self) -> bool {
        self.mode == 2 || self.mode == 3 || (self.mode == 4 && self.update.needs_admin())
    }

    fn start(&mut self, egui_ctx: &egui::Context) {
        let ctx = self.ctx();
        let check = match self.mode {
            0 => self.docker.validate(&ctx),
            1 => self.k8s.validate(&ctx),
            2 => self.win.validate(&ctx),
            3 => self.proxy.validate(&ctx),
            _ => self.update.validate(&ctx),
        }
        .and_then(|_| req("Cartella dei file generati", &self.out_text));
        if let Err(e) = check {
            self.error = Some(e);
            return;
        }
        if self.needs_admin() && !self.admin && !self.dry_run {
            self.error = Some("Questa installazione richiede i privilegi di Amministratore: usa il pulsante 'Riavvia come Amministratore'.".into());
            return;
        }
        self.error = None;
        self.status = None;
        self.log.clear();
        self.show_log = true;
        self.running = true;

        let (tx, rx) = channel();
        self.rx = Some(rx);
        let repaint = egui_ctx.clone();
        ui::set_sink(tx.clone(), Box::new(move || repaint.request_repaint()));

        let mode = self.mode;
        let (d, k, w, p, u) = (self.docker.clone(), self.k8s.clone(), self.win.clone(), self.proxy.clone(), self.update.clone());
        std::thread::spawn(move || {
            let res: Result<Option<(String, String)>, String> = match mode {
                0 => docker::run_install(&ctx, &d).map(|_| None).map_err(|e| e.to_string()),
                1 => k8s::run_install(&ctx, &k).map(|_| None).map_err(|e| e.to_string()),
                2 => winapp::run_install(&ctx, &w).map(|_| None).map_err(|e| e.to_string()),
                3 => proxy::run_install(&ctx, &p).map_err(|e| e.to_string()),
                _ => update::run_update(&ctx, &u).map(|_| None).map_err(|e| e.to_string()),
            };
            let msg = match res {
                Ok(proxy) => Msg::Done { ok: true, text: "Operazione completata".into(), proxy },
                Err(e) => Msg::Done { ok: false, text: e, proxy: None },
            };
            ui::send(msg);
            ui::clear_sink();
        });
    }

    fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        let mut done = false;
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Line(l, t) => self.log.push((l, t)),
                Msg::Done { ok, text, proxy } => {
                    self.running = false;
                    done = true;
                    if !ok {
                        self.log.push((Lvl::Err, text.clone()));
                    }
                    self.status = Some((ok, text));
                    if let Some((url, token)) = proxy {
                        self.docker.ssas.apply_proxy(&url, &token);
                        self.k8s.ssas.apply_proxy(&url, &token);
                        self.log.push((Lvl::Info, "URL e token del proxy precompilati nei form Docker e Kubernetes.".into()));
                    }
                }
            }
        }
        if done {
            self.rx = None;
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.heading("SSAS AI");
        ui.label(RichText::new("Installazione guidata").weak());
        ui.add_space(12.0);
        for (i, (name, desc)) in MODES.iter().enumerate() {
            let sel = self.mode == i;
            let r = ui.add_enabled(
                !self.running,
                egui::SelectableLabel::new(sel, RichText::new(*name).strong().size(16.0)),
            );
            ui.label(RichText::new(*desc).weak().small());
            ui.add_space(4.0);
            if r.clicked() {
                self.mode = i;
                self.error = None;
            }
        }
        ui.add_space(14.0);
        ui.separator();
        ui.label(RichText::new("Cartella del progetto").strong());
        ui.add_enabled_ui(!self.running, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.root_text).desired_width(150.0));
                if ui.button("…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        self.root_text = p.to_string_lossy().into_owned();
                    }
                }
            });
        });
        if self.root().is_some() {
            ui.colored_label(Color32::from_rgb(60, 170, 90), "progetto trovato");
        } else {
            ui.colored_label(Color32::from_rgb(220, 160, 40), "non valida (serve backend/ e frontend/)");
        }
        ui.add_space(8.0);
        ui.label(RichText::new("File generati in").strong());
        ui.add_enabled_ui(!self.running, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.out_text).desired_width(150.0));
                if ui.button("…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        self.out_text = p.to_string_lossy().into_owned();
                    }
                }
            });
        });
        ui.add_space(10.0);
        ui.add_enabled(!self.running, egui::Checkbox::new(&mut self.dry_run, "Solo simulazione"));
        ui.label(RichText::new("scrive i file ma non esegue comandi").weak().small());
        if self.needs_admin() && cfg!(windows) {
            ui.add_space(10.0);
            if self.admin {
                ui.colored_label(Color32::from_rgb(60, 170, 90), "Amministratore: sì");
            } else {
                ui.colored_label(Color32::from_rgb(220, 160, 40), "Servono i privilegi di Amministratore");
                if ui.button("Riavvia come Amministratore").clicked() && relaunch_as_admin().is_ok() {
                    std::process::exit(0);
                }
            }
        }
    }

    fn log_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.running {
                ui.spinner();
                ui.label("Installazione in corso…");
            } else if let Some((ok, t)) = &self.status {
                if *ok {
                    ui.label(RichText::new("✔ Completato").color(Color32::from_rgb(60, 170, 90)).strong());
                } else {
                    ui.label(RichText::new(format!("✘ Non riuscito: {t}")).color(Color32::from_rgb(220, 80, 80)).strong());
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Copia log").clicked() {
                    let txt: Vec<&str> = self.log.iter().map(|l| l.1.as_str()).collect();
                    ui.ctx().copy_text(txt.join("\n"));
                }
                if ui.add_enabled(!self.running, egui::Button::new("← Torna ai parametri")).clicked() {
                    self.show_log = false;
                }
            });
        });
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            for (l, t) in &self.log {
                let (txt, mono) = match l {
                    Lvl::Step => (RichText::new(format!("\n▶ {t}")).strong().color(Color32::from_rgb(70, 160, 220)), false),
                    Lvl::Ok => (RichText::new(format!("✔ {t}")).color(Color32::from_rgb(60, 170, 90)), false),
                    Lvl::Warn => (RichText::new(format!("! {t}")).color(Color32::from_rgb(220, 160, 40)), false),
                    Lvl::Info => (RichText::new(format!("  {t}")).weak(), false),
                    Lvl::Cmd => (RichText::new(format!("$ {t}")).color(Color32::from_rgb(130, 150, 200)), true),
                    Lvl::Out => (RichText::new(format!("    {t}")).weak(), true),
                    Lvl::Err => (RichText::new(format!("✘ {t}")).strong().color(Color32::from_rgb(220, 80, 80)), false),
                };
                ui.label(if mono { txt.monospace() } else { txt });
            }
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();

        egui::SidePanel::left("side").exact_width(240.0).show(ctx, |ui| self.sidebar(ui));

        egui::TopBottomPanel::bottom("actions").show(ctx, |ui| {
            ui.add_space(8.0);
            if let Some(e) = &self.error {
                ui.colored_label(Color32::from_rgb(220, 80, 80), e);
            }
            ui.horizontal(|ui| {
                let label = match (self.mode == 4, self.dry_run) {
                    (true, true) => "Simula aggiornamento",
                    (true, false) => "Aggiorna",
                    (false, true) => "Simula",
                    (false, false) => "Installa",
                };
                let btn = egui::Button::new(RichText::new(label).strong().size(17.0)).min_size(egui::vec2(160.0, 34.0));
                if ui.add_enabled(!self.running && !self.show_log, btn).clicked() {
                    self.start(ctx);
                }
                if self.show_log && !self.running && ui.button("Modifica parametri e riprova").clicked() {
                    self.show_log = false;
                }
                ui.label(RichText::new(format!("{} — {}", MODES[self.mode].0, MODES[self.mode].1)).weak());
            });
            ui.add_space(8.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.show_log {
                self.log_view(ui);
                return;
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(4.0);
                ui.heading(if self.mode == 4 { "Aggiornamento".to_string() } else { format!("Installazione {}", MODES[self.mode].0) });
                let root = self.root().is_some();
                match self.mode {
                    0 => {
                        if !root && self.docker.build {
                            ui.colored_label(Color32::from_rgb(220, 160, 40), "Per la build serve la cartella del progetto (barra laterale).");
                        }
                        self.docker.ui(ui)
                    }
                    1 => self.k8s.ui(ui),
                    2 => {
                        if !cfg!(windows) {
                            ui.colored_label(Color32::from_rgb(220, 160, 40), "Disponibile solo su Windows.");
                        }
                        self.win.ui(ui)
                    }
                    3 => {
                        if !cfg!(windows) {
                            ui.colored_label(Color32::from_rgb(220, 160, 40), "Disponibile solo su Windows (sul server SSAS).");
                        }
                        self.proxy.ui(ui)
                    }
                    _ => {
                        if !root && self.update.target != 1 {
                            ui.colored_label(Color32::from_rgb(220, 160, 40), "Serve la cartella del progetto con il codice nuovo (barra laterale).");
                        }
                        self.update.ui(ui)
                    }
                }
                ui.add_space(20.0);
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1120.0, 800.0]).with_min_inner_size([860.0, 560.0]).with_title("SSAS AI — Installazione guidata"),
        ..Default::default()
    };
    eframe::run_native("SSAS AI Installer", opts, Box::new(|cc| {
        cc.egui_ctx.set_zoom_factor(1.1);
        Ok(Box::new(App::new()))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Esegue tutte le modalità in simulazione e scrive i file in SSAS_TEST_OUT.
    #[test]
    fn dry_run_all_modes() {
        let Ok(out) = std::env::var("SSAS_TEST_OUT") else { return };
        let root = find_root();
        let ctx = Ctx { root: root.clone(), dry_run: true, out_base: PathBuf::from(out) };

        let mut d = docker::DockerForm::new(root.is_some());
        d.ssas.apply_proxy("http://10.0.0.5:9000", "tok$en#1");
        d.ssas.database = "Db1".into();
        d.llm.key = "sk-test".into();
        d.validate(&ctx).unwrap();
        docker::run_install(&ctx, &d).unwrap();

        let mut k = k8s::K8sForm::new();
        k.host = "ssas.example.com".into();
        k.email = "a@b.it".into();
        k.private_registry = true;
        k.reg_user = "u".into();
        k.reg_pass = "p".into();
        k.ssas.apply_proxy("http://10.0.0.5:9000", "tok");
        k.ssas.database = "Db1".into();
        k.app.webauthn = true;
        k.app.webauthn_host = "ssas.example.com".into();
        k.contexts = vec!["docker-desktop".into()];
        k.validate(&ctx).unwrap();
        k8s::run_install(&ctx, &k).unwrap();

        // aggiornamenti in simulazione
        let mut u = update::UpdateForm::new(&ctx.out_base);
        u.target = 0;
        u.validate(&ctx).unwrap();
        update::run_update(&ctx, &u).unwrap();
        u.target = 1;
        u.validate(&ctx).unwrap_or(());
        update::run_update(&ctx, &u).unwrap();

        let mut w = winapp::WinForm::new();
        w.server = "SRV".into();
        w.database = "DB".into();
        w.svc_account = r"DOM\svc".into();
        if cfg!(windows) && root.is_some() {
            w.validate(&ctx).unwrap();
            winapp::run_install(&ctx, &w).unwrap();
            let mut p = proxy::ProxyForm::new();
            p.database = "DB".into();
            p.validate(&ctx).unwrap();
            proxy::run_install(&ctx, &p).unwrap();
        }
    }
}
