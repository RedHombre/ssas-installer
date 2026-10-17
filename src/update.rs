//! Aggiornamento di un'installazione esistente (Docker, Kubernetes, Windows, proxy SSAS).

use crate::common::*;
use crate::gui::*;
use crate::k8s::{kc, kcap, list_contexts};
use crate::ui::*;
use eframe::egui::Ui;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const TARGETS: [&str; 4] = [
    "Docker (docker compose)",
    "Kubernetes",
    "Windows (app nativa)",
    "Proxy SSAS (server Windows)",
];

#[derive(Clone)]
pub struct UpdateForm {
    pub target: usize,
    // Docker
    docker_dir: String,
    docker_backup: bool,
    // Kubernetes
    k_contexts: Vec<String>,
    k_ctx: usize,
    k_ns: String,
    k_build: bool,
    k_image: String,
    k_rollback: bool,
    // Windows
    w_dir: String,
    w_service: String,
    w_no_backup: bool,
    // Proxy
    p_dir: String,
    p_service: String,
}

impl UpdateForm {
    pub fn new(out_base: &Path) -> Self {
        let (k_contexts, k_ctx) = list_contexts();
        let w_dir = [r"C:\SSas_ai", r"C:\ssas-ai"].iter().find(|d| Path::new(d).join(".venv").is_dir()).unwrap_or(&r"C:\ssas-ai");
        Self {
            target: 0,
            docker_dir: out_base.join("ssas-ai-docker").to_string_lossy().into_owned(),
            docker_backup: true,
            k_contexts,
            k_ctx,
            k_ns: "ssas-ai".into(),
            k_build: false,
            k_image: String::new(),
            k_rollback: false,
            w_dir: (*w_dir).into(),
            w_service: "ssas-ai".into(),
            w_no_backup: false,
            p_dir: r"C:\service".into(),
            p_service: "SsasProxy".into(),
        }
    }

    pub fn needs_admin(&self) -> bool {
        self.target >= 2
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        section(ui, "Cosa aggiornare", |ui| {
            grid(ui, "u-target", |ui| combo(ui, "Installazione", &mut self.target, &TARGETS));
        });
        match self.target {
            0 => section(ui, "Docker", |ui| {
                grid(ui, "u-docker", |ui| {
                    path(ui, "Cartella con docker-compose.yml", &mut self.docker_dir, "", Pick::Dir);
                    check(ui, "Backup database", &mut self.docker_backup, "salva un dump SQL prima di aggiornare (se il database è nel compose)");
                });
                note(ui, "Ricostruisce l'immagine dai sorgenti, oppure la riscarica dal registry, e ricrea i container. .env e volumi restano invariati.");
            }),
            1 => section(ui, "Kubernetes", |ui| {
                grid(ui, "u-k8s", |ui| {
                    ui.label("Cluster (context kubectl)");
                    ui.horizontal(|ui| {
                        if self.k_contexts.is_empty() {
                            ui.label("nessun context trovato");
                        } else {
                            let sel = self.k_contexts[self.k_ctx.min(self.k_contexts.len() - 1)].clone();
                            eframe::egui::ComboBox::from_id_salt("uctx").selected_text(sel).width(360.0).show_ui(ui, |ui| {
                                for (i, c) in self.k_contexts.clone().iter().enumerate() {
                                    ui.selectable_value(&mut self.k_ctx, i, c);
                                }
                            });
                        }
                        if ui.button("Aggiorna").clicked() {
                            let (v, i) = list_contexts();
                            self.k_contexts = v;
                            self.k_ctx = i;
                        }
                    });
                    ui.end_row();
                    text(ui, "Namespace", &mut self.k_ns, "ssas-ai");
                    check(ui, "Torna alla versione precedente", &mut self.k_rollback, "annulla l'ultimo aggiornamento (kubectl rollout undo)");
                    if !self.k_rollback {
                        check(ui, "Build e push", &mut self.k_build, "costruisci l'immagine dai sorgenti e pubblicala prima di aggiornare");
                        text(ui, "Nuova immagine", &mut self.k_image, "registro/nome:tag — vuoto = riavvia con l'immagine attuale (:latest)");
                    }
                });
            }),
            2 => section(ui, "Windows", |ui| {
                grid(ui, "u-win", |ui| {
                    path(ui, "Cartella di installazione", &mut self.w_dir, r"C:\SSas_ai", Pick::Dir);
                    text(ui, "Nome del servizio", &mut self.w_service, "ssas-ai");
                    check(ui, "Senza backup", &mut self.w_no_backup, "salta il backup di database, data e .env (sconsigliato)");
                });
                let v = |d: &Path| std::fs::read_to_string(d.join("VERSION")).map(|s| s.trim().to_string()).unwrap_or_else(|_| "?".into());
                note(ui, &format!("Versione installata: {}", v(Path::new(self.w_dir.trim()))));
                note(ui, "Copia backend e frontend del progetto in questa cartella, aggiorna le dipendenze e riavvia il servizio. .env, data e logs restano.");
            }),
            _ => section(ui, "Proxy SSAS", |ui| {
                grid(ui, "u-proxy", |ui| {
                    path(ui, "Cartella del proxy", &mut self.p_dir, r"C:\service", Pick::Dir);
                    text(ui, "Nome servizio/attività", &mut self.p_service, "SsasProxy");
                });
                note(ui, "Sostituisce ssas_proxy.py (copia .bak) e riavvia il proxy. config.json e token restano invariati.");
            }),
        }
    }

    pub fn validate(&self, ctx: &Ctx) -> Result<(), String> {
        match self.target {
            0 => {
                if !Path::new(self.docker_dir.trim()).join("docker-compose.yml").is_file() {
                    return Err("docker-compose.yml non trovato nella cartella indicata".into());
                }
                Ok(())
            }
            1 => {
                if self.k_contexts.is_empty() && !ctx.dry_run {
                    return Err("Nessun context kubectl disponibile".into());
                }
                valid_k8s_name("Namespace", &self.k_ns)?;
                if self.k_build && !self.k_rollback {
                    root_of(ctx, "serve per la build")?;
                    req("Nuova immagine", &self.k_image)?;
                }
                Ok(())
            }
            2 => {
                if !cfg!(windows) {
                    return Err("L'aggiornamento nativo è disponibile solo su Windows".into());
                }
                let root = root_of(ctx, "contiene gli script e il codice nuovo")?;
                if !root.join("installazione").join("windows-app").join("update.ps1").is_file() {
                    return Err("update.ps1 non trovato in installazione/windows-app".into());
                }
                if !Path::new(self.w_dir.trim()).join(".venv").join("Scripts").join("python.exe").is_file() {
                    return Err("Nessuna installazione trovata nella cartella indicata (manca .venv)".into());
                }
                if root == PathBuf::from(self.w_dir.trim()) {
                    return Err("La cartella di installazione coincide con quella del progetto".into());
                }
                valid_svc_name("Nome del servizio", &self.w_service)
            }
            _ => {
                if !cfg!(windows) {
                    return Err("L'aggiornamento del proxy è disponibile solo su Windows".into());
                }
                let root = root_of(ctx, "contiene il nuovo ssas_proxy.py")?;
                if !root.join("installazione").join("windows-proxy").join("ssas_proxy.py").is_file() {
                    return Err("ssas_proxy.py non trovato in installazione/windows-proxy".into());
                }
                if !Path::new(self.p_dir.trim()).join("ssas_proxy.py").is_file() {
                    return Err("ssas_proxy.py non trovato nella cartella del proxy: non risulta installato lì".into());
                }
                valid_svc_name("Nome servizio/attività", &self.p_service)
            }
        }
    }
}

pub fn run_update(ctx: &Ctx, f: &UpdateForm) -> Res<()> {
    match f.target {
        0 => docker(ctx, f),
        1 => k8s(ctx, f),
        2 => windows(ctx, f),
        _ => proxy(ctx, f),
    }
}

// ───────────────────────── Docker ─────────────────────────

fn env_value(env: &str, key: &str) -> Option<String> {
    env.lines()
        .find_map(|l| l.strip_prefix(&format!("{key}=")))
        .map(|v| v.trim().trim_matches('\'').trim_matches('"').to_string())
}

fn docker(ctx: &Ctx, f: &UpdateForm) -> Res<()> {
    step("Verifica Docker");
    if !(have("docker", &["info"]) && have("docker", &["compose", "version"])) {
        if ctx.dry_run {
            warn("Docker non disponibile (ignorato: simulazione)");
        } else {
            return err("Docker o Docker Compose non disponibili (il daemon è avviato?)");
        }
    } else {
        ok("Docker disponibile");
    }
    let dir = PathBuf::from(f.docker_dir.trim());
    let compose = dir.join("docker-compose.yml");
    let text = std::fs::read_to_string(&compose)?;
    let env = std::fs::read_to_string(dir.join(".env")).unwrap_or_default();
    let cf = compose.to_string_lossy().into_owned();

    if f.docker_backup && text.contains("postgres:") {
        if let (Some(user), Some(db)) = (env_value(&env, "POSTGRES_USER"), env_value(&env, "POSTGRES_DB")) {
            step("Backup del database");
            let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let file = dir.join(format!("backup-{secs}.sql"));
            if run_to_file(ctx, "docker", &["compose", "-f", &cf, "exec", "-T", "postgres", "pg_dump", "-U", &user, "-d", &db], &file)? {
                ok(&format!("backup: {}", file.display()));
            } else {
                let _ = std::fs::remove_file(&file);
                return err("Backup non riuscito: aggiornamento annullato (i container sono ancora in esecuzione?)");
            }
        } else {
            warn("Credenziali del database non trovate in .env: backup saltato");
        }
    }

    step("Aggiornamento dell'immagine");
    let build = text.contains("build:");
    let ok_ = if build {
        info("Immagine costruita dai sorgenti: rifaccio la build con il codice attuale del progetto.");
        run_in(ctx, &dir, "docker", &["compose", "build", "--pull"])?
    } else {
        run_in(ctx, &dir, "docker", &["compose", "pull"])?
    };
    if !ok_ {
        return err(if build { "Build non riuscita: l'installazione attuale non è stata toccata" } else { "Download dell'immagine non riuscito (docker login?)" });
    }

    step("Riavvio dei container");
    if !run_in(ctx, &dir, "docker", &["compose", "up", "-d"])? {
        return err("docker compose up non riuscito");
    }
    if ctx.dry_run {
        ok("Simulazione completata: nessun comando eseguito.");
        return Ok(());
    }
    step("Attesa che l'applicazione sia pronta");
    let mut healthy = false;
    for _ in 0..45 {
        std::thread::sleep(Duration::from_secs(2));
        match capture_any("docker", &["inspect", "--format", "{{.State.Health.Status}}", "ssas-ai"]).trim() {
            "healthy" => {
                healthy = true;
                break;
            }
            "unhealthy" => break,
            _ => {}
        }
    }
    if healthy {
        ok("Aggiornamento completato: container healthy");
        run(ctx, "docker", &["image", "prune", "-f"])?;
        Ok(())
    } else {
        info(&capture_any("docker", &["logs", "--tail", "25", "ssas-ai"]));
        err("Il container non è diventato healthy dopo l'aggiornamento: vedi il log sopra")
    }
}

// ───────────────────────── Kubernetes ─────────────────────────

fn k8s(ctx: &Ctx, f: &UpdateForm) -> Res<()> {
    let kctx = f.k_contexts.get(f.k_ctx).cloned().unwrap_or_else(|| "(dry-run)".into());
    let ns = f.k_ns.trim().to_string();
    step(&format!("Cluster '{kctx}', namespace '{ns}'"));
    if !ctx.dry_run && !have("kubectl", &["--context", &kctx, "-n", &ns, "get", "deployment", "ssas-ai"]) {
        return err(format!("Deployment 'ssas-ai' non trovato nel namespace '{ns}' del cluster '{kctx}'"));
    }
    if f.k_rollback {
        step("Ritorno alla versione precedente");
        if !kc(ctx, &kctx, &["-n", &ns, "rollout", "undo", "deployment/ssas-ai"])? {
            return err("rollout undo non riuscito");
        }
    } else {
        let image = f.k_image.trim();
        if f.k_build {
            step("Build e push dell'immagine");
            let rp = clean_path(&root_of(ctx, "build")?);
            let df = format!("{}/installazione/docker/Dockerfile", rp.replace('\\', "/"));
            if !run(ctx, "docker", &["build", "-f", &df, "-t", image, &rp])? {
                return err("Build non riuscita: il cluster non è stato toccato");
            }
            if !run(ctx, "docker", &["push", image])? {
                return err("Push non riuscito (docker login sul registry?)");
            }
        }
        step("Aggiornamento del deployment");
        let done = if image.is_empty() {
            info("Nessuna nuova immagine: riavvio i pod (con :latest e imagePullPolicy Always scaricano l'ultima).");
            kc(ctx, &kctx, &["-n", &ns, "rollout", "restart", "deployment/ssas-ai"])?
        } else {
            kc(ctx, &kctx, &["-n", &ns, "set", "image", "deployment/ssas-ai", &format!("ssas-ai={image}")])?
        };
        if !done {
            return err("Aggiornamento del deployment non riuscito");
        }
    }
    if ctx.dry_run {
        ok("Simulazione completata: nessun comando eseguito.");
        return Ok(());
    }
    step("Attesa del rollout");
    let rolled = kc(ctx, &kctx, &["-n", &ns, "rollout", "status", "deployment/ssas-ai", "--timeout=420s"])?;
    kc(ctx, &kctx, &["-n", &ns, "get", "pods", "-o", "wide"])?;
    if rolled {
        ok("Aggiornamento completato");
        info(&format!("Per tornare indietro: kubectl --context {kctx} -n {ns} rollout undo deployment/ssas-ai (o la casella nel form)"));
        Ok(())
    } else {
        info(&kcap(&kctx, &["-n", &ns, "logs", "deploy/ssas-ai", "--tail=25"]));
        err("Il rollout non si è completato: usa «Torna alla versione precedente» se serve")
    }
}

// ───────────────────────── Windows ─────────────────────────

fn windows(ctx: &Ctx, f: &UpdateForm) -> Res<()> {
    let root = root_of(ctx, "update.ps1")?;
    let script = root.join("installazione").join("windows-app").join("update.ps1");
    step("Aggiornamento dell'app Windows");
    info("update.ps1 ferma il servizio, fa il backup (può chiedere la password del database), copia i file e riavvia.");
    let mut p: Vec<(&str, Value)> = vec![("InstallDir", json!(f.w_dir.trim())), ("ServiceName", json!(f.w_service.trim()))];
    if f.w_no_backup {
        p.push(("NoBackup", json!(true)));
    }
    if !run_ps(ctx, &script, &p)? {
        return err("update.ps1 non è terminato correttamente (vedi le righe sopra)");
    }
    if ctx.dry_run {
        ok("Simulazione completata: nessun comando eseguito.");
        return Ok(());
    }
    step("Verifica");
    let env = std::fs::read_to_string(Path::new(f.w_dir.trim()).join(".env")).unwrap_or_default();
    let port: u16 = env_value(&env, "PORT").and_then(|p| p.parse().ok()).unwrap_or(8000);
    if wait_http("localhost", port, "/healthz", 30) {
        ok(&format!("Aggiornamento completato: l'app risponde su http://localhost:{port}"));
        Ok(())
    } else {
        let log = std::fs::read_to_string(Path::new(f.w_dir.trim()).join("logs").join("ssas-ai.log")).unwrap_or_default();
        let lines: Vec<&str> = log.lines().filter(|l| !l.trim().is_empty()).collect();
        for l in &lines[lines.len().saturating_sub(15)..] {
            out(l);
        }
        err(format!("L'app non risponde su porta {port} dopo l'aggiornamento (HTTPS? vedi il log sopra)"))
    }
}

// ───────────────────────── Proxy ─────────────────────────

fn proxy(ctx: &Ctx, f: &UpdateForm) -> Res<()> {
    let root = root_of(ctx, "ssas_proxy.py")?;
    let new = root.join("installazione").join("windows-proxy").join("ssas_proxy.py");
    let dir = PathBuf::from(f.p_dir.trim());
    let dst = dir.join("ssas_proxy.py");
    let svc = f.p_service.trim();
    let ps = |c: &str| run(ctx, "powershell.exe", &["-NoProfile", "-Command", c]);

    step("Arresto del proxy");
    ps(&format!("Stop-Service '{svc}' -Force -ErrorAction SilentlyContinue; Stop-ScheduledTask -TaskName '{svc}' -ErrorAction SilentlyContinue; Start-Sleep 2"))?;

    step("Sostituzione di ssas_proxy.py");
    if ctx.dry_run {
        info(&format!("(simulazione) copierei {} in {}", new.display(), dst.display()));
    } else {
        std::fs::copy(&dst, dir.join("ssas_proxy.py.bak"))?;
        ok("copia di sicurezza: ssas_proxy.py.bak");
        if let Err(e) = std::fs::copy(&new, &dst) {
            let _ = ps(&format!("Start-Service '{svc}' -ErrorAction SilentlyContinue; Start-ScheduledTask -TaskName '{svc}' -ErrorAction SilentlyContinue"));
            return err(format!("Copia non riuscita ({e}): il proxy è stato riavviato con il file precedente"));
        }
        ok("ssas_proxy.py aggiornato (config.json e token invariati)");
    }

    step("Riavvio del proxy");
    ps(&format!("Start-Service '{svc}' -ErrorAction SilentlyContinue; Start-ScheduledTask -TaskName '{svc}' -ErrorAction SilentlyContinue"))?;
    if ctx.dry_run {
        ok("Simulazione completata: nessun comando eseguito.");
        return Ok(());
    }
    let port = std::fs::read_to_string(dir.join("config.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["proxy"]["port"].as_u64())
        .unwrap_or(9000) as u16;
    if wait_http("localhost", port, "/health", 20) {
        ok(&format!("Proxy aggiornato: risponde su http://localhost:{port}/health"));
        Ok(())
    } else {
        warn("Il proxy non risponde: per tornare indietro rinomina ssas_proxy.py.bak in ssas_proxy.py e riavvialo.");
        err("Il proxy non risponde dopo l'aggiornamento")
    }
}
