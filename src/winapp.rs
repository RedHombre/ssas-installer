//! Installazione nativa su Windows: l'app gira come servizio e si collega
//! direttamente a SSAS con ADOMD.NET (nessun proxy). Usa gli script
//! `installazione/windows-app/*.ps1` del progetto.

use crate::common::*;
use crate::gui::*;
use crate::ui::*;
use eframe::egui::Ui;
use serde_json::{json, Value};

#[derive(Clone)]
pub struct WinForm {
    pub install_dir: String,
    pub service: String,
    pub as_service: bool,
    pub firewall: bool,
    pub demo: bool,
    pub server: String,
    pub database: String,
    /// 0 account di servizio, 1 LocalSystem, 2 utente/password SSAS
    pub auth: usize,
    pub svc_account: String,
    pub ssas_user: String,
    pub ssas_pass: String,
    pub local_pg: bool,
    pub db_name: String,
    pub db_user: String,
    pub db_port: String,
    pub db_url: String,
    pub https: bool,
    pub cert: String,
    pub key: String,
    pub llm: LlmForm,
    pub app: AppForm,
}

impl WinForm {
    pub fn new() -> Self {
        Self {
            install_dir: r"C:\ssas-ai".into(),
            service: "ssas-ai".into(),
            as_service: true,
            firewall: true,
            demo: false,
            server: String::new(),
            database: String::new(),
            auth: 0,
            svc_account: String::new(),
            ssas_user: String::new(),
            ssas_pass: String::new(),
            local_pg: true,
            db_name: "ssasai".into(),
            db_user: "ssasai".into(),
            db_port: "5432".into(),
            db_url: String::new(),
            https: false,
            cert: String::new(),
            key: String::new(),
            llm: LlmForm::new(""),
            app: AppForm::new(8000),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        section(ui, "Installazione", |ui| {
            grid(ui, "w-inst", |ui| {
                path(ui, "Cartella di installazione", &mut self.install_dir, r"C:\ssas-ai", Pick::Dir);
                text(ui, "Nome del servizio", &mut self.service, "ssas-ai");
                check(ui, "Servizio Windows", &mut self.as_service, "registra l'app con avvio automatico (altrimenti avvio manuale con start.bat)");
                check(ui, "Firewall", &mut self.firewall, "apri la porta nel firewall di Windows");
            });
        });
        section(ui, "Connessione a SSAS", |ui| {
            grid(ui, "w-ssas", |ui| {
                check(ui, "Modalità DEMO", &mut self.demo, "dati simulati, nessun SSAS");
                if !self.demo {
                    text(ui, "Server SSAS", &mut self.server, "SRVBI, SRVBI\\TABULAR, SRVBI:2383");
                    text(ui, "Database OLAP/Tabular", &mut self.database, "");
                    combo(
                        ui,
                        "Autenticazione",
                        &mut self.auth,
                        &[
                            "Windows con account di servizio dedicato (DOMINIO\\utente) — consigliato",
                            "Windows con LocalSystem (l'app si presenta come DOMINIO\\SERVER$)",
                            "Utente e password SSAS espliciti",
                        ],
                    );
                    match self.auth {
                        0 => text(ui, "Account del servizio", &mut self.svc_account, "DOMINIO\\svc-ssasai (la password sarà chiesta da Windows)"),
                        2 => {
                            text(ui, "Utente SSAS", &mut self.ssas_user, "");
                            secret(ui, "Password SSAS", &mut self.ssas_pass, "");
                        }
                        _ => {}
                    }
                }
            });
        });
        section(ui, "Database PostgreSQL", |ui| {
            grid(ui, "w-db", |ui| {
                let mut idx = usize::from(!self.local_pg);
                combo(ui, "Database", &mut idx, &["PostgreSQL 18 locale (lo installo e creo database e utente)", "PostgreSQL esistente (DATABASE_URL)"]);
                self.local_pg = idx == 0;
                if self.local_pg {
                    text(ui, "Nome database", &mut self.db_name, "");
                    text(ui, "Utente database", &mut self.db_user, "");
                    text(ui, "Porta PostgreSQL", &mut self.db_port, "5432");
                } else {
                    text(ui, "DATABASE_URL", &mut self.db_url, "postgresql://utente:password@host:5432/db");
                }
            });
            if self.local_pg {
                note(ui, "La password dell'utente viene generata e salvata in .env. Gli script chiederanno la password dell'utente 'postgres'.");
            }
        });
        section(ui, "HTTPS", |ui| {
            grid(ui, "w-https", |ui| {
                check(ui, "HTTPS diretto", &mut self.https, "servi l'app in HTTPS con certificato e chiave PEM");
                if self.https {
                    path(ui, "Certificato PEM", &mut self.cert, "", Pick::File);
                    path(ui, "Chiave privata PEM", &mut self.key, "", Pick::File);
                }
            });
        });
        self.llm.ui(ui, false);
        self.app.ui(ui, Some("Porta dell'app"));
    }

    pub fn validate(&self, ctx: &Ctx) -> Result<(), String> {
        if !cfg!(windows) {
            return Err("L'installazione nativa è disponibile solo su Windows".into());
        }
        let root = root_of(ctx, "contiene gli script di installazione")?;
        if !root.join("installazione").join("windows-app").join("install.ps1").is_file() {
            return Err("install.ps1 non trovato in installazione/windows-app".into());
        }
        req("Cartella di installazione", &self.install_dir)?;
        valid_svc_name("Nome del servizio", &self.service)?;
        if !self.demo {
            req("Server SSAS", &self.server)?;
            req("Database SSAS", &self.database)?;
            match self.auth {
                0 => req("Account del servizio", &self.svc_account)?,
                2 => {
                    req("Utente SSAS", &self.ssas_user)?;
                    req("Password SSAS", &self.ssas_pass)?;
                }
                _ => {}
            }
        }
        if self.local_pg {
            valid_db_ident("Nome database", &self.db_name)?;
            valid_db_ident("Utente database", &self.db_user)?;
            num::<u16>("Porta PostgreSQL", &self.db_port, 1, 65535)?;
        } else {
            valid_pg_url("DATABASE_URL", &self.db_url)?;
        }
        if self.https {
            for (l, p) in [("Certificato", &self.cert), ("Chiave privata", &self.key)] {
                if !std::path::Path::new(p.trim()).is_file() {
                    return Err(format!("{l}: file non trovato"));
                }
            }
        }
        self.llm.validate()?;
        self.app.validate(true)
    }
}

pub fn run_install(ctx: &Ctx, f: &WinForm) -> Res<()> {
    let root = root_of(ctx, "script di installazione")?;
    let dir = root.join("installazione").join("windows-app");
    let install_dir = f.install_dir.trim().to_string();
    let port = f.app.port_u16();

    // ── prerequisiti ──
    step("Prerequisiti (Python, .NET, PowerShell 7, NSSM, PostgreSQL)");
    let mut pre: Vec<(&str, Value)> = vec![];
    if f.local_pg {
        pre.push(("WithPostgres", json!(true)));
        pre.push(("PostgresPort", json!(f.db_port.trim().parse::<u16>().unwrap_or(5432))));
    }
    if !run_ps(ctx, &dir.join("setup-prerequisites.ps1"), &pre)? {
        return err("Installazione dei prerequisiti non riuscita (vedi la finestra PowerShell)");
    }

    // ── .env preconfigurato ──
    // install.ps1 conserva i valori già presenti in .env: chiave LLM, segreti e
    // limiti vengono così impostati senza ulteriori domande.
    step("Configurazione preliminare (.env)");
    let mut env = EnvFile::default();
    env.set("AUTH_SECRET_KEY", &f.app.auth_secret);
    env.extend(&f.llm.env());
    env.extend(&f.app.env_public());
    if !f.demo && f.auth == 2 {
        env.set("SSAS_USER", f.ssas_user.trim());
        env.set("SSAS_PASSWORD", &f.ssas_pass);
    }
    let env_path = std::path::Path::new(&install_dir).join(".env");
    if env_path.exists() {
        warn(".env già presente: lo lascio invariato (verrà aggiornato solo da install.ps1)");
    } else if ctx.dry_run {
        info(&format!("(simulazione) scriverei {}", env_path.display()));
    } else {
        write_file(&env_path, &env.render().replace('\n', "\r\n"), true)?;
    }

    // ── installazione ──
    step("Installazione dell'app");
    let mut p: Vec<(&str, Value)> = vec![
        ("InstallDir", json!(install_dir)),
        ("Port", json!(port)),
        ("ServiceName", json!(f.service.trim())),
        ("LlmProvider", json!(f.llm.name())),
    ];
    if !f.llm.model.trim().is_empty() {
        p.push(("LlmModel", json!(f.llm.model.trim())));
    }
    if f.demo {
        p.push(("Demo", json!(true)));
    } else {
        p.push(("SsasServer", json!(f.server.trim())));
        p.push(("SsasDatabase", json!(f.database.trim())));
        if f.auth == 0 {
            p.push(("ServiceAccount", json!(f.svc_account.trim())));
        }
    }
    if f.local_pg {
        p.push(("DbName", json!(f.db_name.trim())));
        p.push(("DbUser", json!(f.db_user.trim())));
        p.push(("DbPort", json!(f.db_port.trim().parse::<u16>().unwrap_or(5432))));
    } else {
        p.push(("DatabaseUrl", json!(f.db_url.trim())));
    }
    if f.https {
        p.push(("CertFile", json!(f.cert.trim())));
        p.push(("KeyFile", json!(f.key.trim())));
    }
    if !f.as_service {
        p.push(("NoService", json!(true)));
    }
    if !f.firewall {
        p.push(("NoFirewall", json!(true)));
    }
    if !run_ps(ctx, &dir.join("install.ps1"), &p)? {
        return err("install.ps1 non è terminato correttamente (vedi la finestra PowerShell)");
    }
    if ctx.dry_run {
        ok("Simulazione completata: nessun comando eseguito.");
        return Ok(());
    }

    // ── verifica ──
    step("Verifica");
    if !f.as_service {
        info("Servizio non registrato: avvia l'app con start.bat nella cartella di installazione.");
    } else if f.https {
        info("App in HTTPS: controllo HTTP non applicabile, apri l'indirizzo nel browser.");
    } else if wait_http("localhost", port, "/healthz", 20) {
        ok(&format!("L'app risponde su porta {port}"));
    } else {
        warn("L'app non risponde. Ultime righe del suo log:");
        let log = std::fs::read_to_string(std::path::Path::new(&install_dir).join("logs").join("ssas-ai.log")).unwrap_or_default();
        let lines: Vec<&str> = log.lines().filter(|l| !l.trim().is_empty()).collect();
        for l in &lines[lines.len().saturating_sub(15)..] {
            out(l);
        }
        return err(format!("L'app non risponde su porta {port}: vedi il log sopra ({install_dir}\\logs\\ssas-ai.log)"));
    }
    let proto = if f.https { "https" } else { "http" };
    ok(&format!("SSAS AI installato in {install_dir} — {proto}://localhost:{port}"));
    info("Utente 'admin': la password iniziale è nel log dell'app (cartella logs) al primo avvio.");
    Ok(())
}
