//! Installazione del proxy REST SSAS (ssas_proxy.py) sul server Windows con SSAS.
//! Serve ai backend Linux (Docker/Kubernetes), che non possono usare ADOMD.NET.

use crate::common::*;
use crate::gui::*;
use crate::ui::*;
use eframe::egui::Ui;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const ADOMD_CANDIDATES: &[&str] = &[
    r"C:\Program Files\Microsoft Power BI Report Server\PBIRS\ReportServer\bin\Microsoft.AnalysisServices.AdomdClient.dll",
    r"C:\Program Files\Microsoft SQL Server\160\SDK\Assemblies\Microsoft.AnalysisServices.AdomdClient.dll",
    r"C:\Program Files\Microsoft SQL Server\150\SDK\Assemblies\Microsoft.AnalysisServices.AdomdClient.dll",
    r"C:\Program Files\Microsoft SQL Server\140\SDK\Assemblies\Microsoft.AnalysisServices.AdomdClient.dll",
    r"C:\Program Files\Microsoft SQL Server\130\SDK\Assemblies\Microsoft.AnalysisServices.AdomdClient.dll",
    r"C:\Program Files (x86)\Microsoft.NET\ADOMD.NET\160\Microsoft.AnalysisServices.AdomdClient.dll",
    r"C:\Program Files (x86)\Microsoft.NET\ADOMD.NET\150\Microsoft.AnalysisServices.AdomdClient.dll",
    r"C:\Program Files (x86)\Microsoft.NET\ADOMD.NET\140\Microsoft.AnalysisServices.AdomdClient.dll",
    r"C:\Program Files (x86)\Microsoft.NET\ADOMD.NET\130\Microsoft.AnalysisServices.AdomdClient.dll",
];

#[derive(Clone)]
pub struct ProxyForm {
    pub dir: String,
    pub port: String,
    pub as_service: bool,
    pub svc_name: String,
    pub server: String,
    pub database: String,
    pub explicit_auth: bool,
    pub user: String,
    pub password: String,
    pub allowed: String,
    pub dll: String,
    pub use_token: bool,
    pub token: String,
    pub timeout: String,
    pub max_rows: String,
}

impl ProxyForm {
    pub fn new() -> Self {
        let dll = ADOMD_CANDIDATES.iter().find(|p| Path::new(p).is_file()).unwrap_or(&ADOMD_CANDIDATES[0]);
        Self {
            dir: r"C:\service".into(),
            port: "9000".into(),
            as_service: true,
            svc_name: "SsasProxy".into(),
            server: "localhost".into(),
            database: String::new(),
            explicit_auth: false,
            user: String::new(),
            password: String::new(),
            allowed: String::new(),
            dll: (*dll).into(),
            use_token: true,
            token: rand_hex(32),
            timeout: "120".into(),
            max_rows: "50000".into(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        section(ui, "Installazione", |ui| {
            grid(ui, "p-inst", |ui| {
                path(ui, "Cartella del proxy", &mut self.dir, r"C:\service", Pick::Dir);
                text(ui, "Porta", &mut self.port, "9000");
                let mut idx = usize::from(!self.as_service);
                combo(ui, "Esecuzione", &mut idx, &["Servizio Windows con NSSM (consigliato)", "Attività pianificata all'avvio (nessuno strumento extra)"]);
                self.as_service = idx == 0;
                text(ui, "Nome servizio/attività", &mut self.svc_name, "SsasProxy");
            });
        });
        section(ui, "Server SSAS", |ui| {
            grid(ui, "p-ssas", |ui| {
                text(ui, "Server SSAS", &mut self.server, "localhost");
                text(ui, "Database predefinito", &mut self.database, "");
                text(ui, "Database ammessi", &mut self.allowed, "elenco separato da virgole, vuoto = tutti");
                check(ui, "Credenziali esplicite", &mut self.explicit_auth, "invece dell'autenticazione Windows integrata");
                if self.explicit_auth {
                    text(ui, "Utente SSAS", &mut self.user, "");
                    secret(ui, "Password SSAS", &mut self.password, "");
                }
                path(ui, "DLL ADOMD.NET", &mut self.dll, "", Pick::File);
            });
            if !Path::new(self.dll.trim()).is_file() {
                note(ui, "DLL non trovata: serve Power BI Report Server, SSMS o ADOMD.NET client. Puoi installarla dopo e correggere config.json.");
            }
        });
        section(ui, "Sicurezza e limiti", |ui| {
            grid(ui, "p-sec", |ui| {
                check(ui, "Token condiviso", &mut self.use_token, "proteggi il proxy (consigliato)");
                if self.use_token {
                    ui.label("Token");
                    ui.horizontal(|ui| {
                        ui.add(eframe::egui::TextEdit::singleline(&mut self.token).desired_width(420.0));
                        if ui.button("Rigenera").clicked() {
                            self.token = rand_hex(32);
                        }
                    });
                    ui.end_row();
                }
                text(ui, "Timeout query (s)", &mut self.timeout, "");
                text(ui, "Righe massime per query", &mut self.max_rows, "");
            });
        });
    }

    fn allowed_list(&self) -> Vec<String> {
        self.allowed.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    }

    pub fn validate(&self, ctx: &Ctx) -> Result<(), String> {
        if !cfg!(windows) {
            return Err("Il proxy SSAS si installa solo su Windows".into());
        }
        let root = root_of(ctx, "contiene ssas_proxy.py e gli script")?;
        if !root.join("installazione").join("windows-proxy").join("ssas_proxy.py").is_file() {
            return Err("ssas_proxy.py non trovato in installazione/windows-proxy".into());
        }
        req("Cartella del proxy", &self.dir)?;
        num::<u16>("Porta", &self.port, 1, 65535)?;
        valid_svc_name("Nome servizio/attività", &self.svc_name)?;
        req("Server SSAS", &self.server)?;
        req("Database predefinito", &self.database)?;
        for d in self.allowed_list().iter().chain(std::iter::once(&self.database)) {
            if d.contains([';', '=', '\'', '"']) {
                return Err(format!("Nome database non valido (caratteri ; = ' \" non ammessi): {d}"));
            }
        }
        if self.explicit_auth {
            req("Utente SSAS", &self.user)?;
            req("Password SSAS", &self.password)?;
        }
        if self.use_token {
            req("Token", &self.token)?;
        }
        num::<u32>("Timeout", &self.timeout, 1, 3600)?;
        num::<u32>("Righe massime", &self.max_rows, 1, 5_000_000)?;
        Ok(())
    }
}

/// Restituisce (url, token) del proxy installato.
pub fn run_install(ctx: &Ctx, f: &ProxyForm) -> Res<Option<(String, String)>> {
    let root = root_of(ctx, "ssas_proxy.py")?;
    let src = root.join("installazione").join("windows-proxy");
    let dir = PathBuf::from(f.dir.trim());
    let port: u16 = f.port.trim().parse().unwrap_or(9000);
    let token = if f.use_token { f.token.trim().to_string() } else { String::new() };
    let svc = f.svc_name.trim().to_string();
    let (server, database) = (f.server.trim().to_string(), f.database.trim().to_string());

    step("Scrittura dei file");
    let cfg = json!({
        "proxy": { "host": "0.0.0.0", "port": port, "token": token, "allowed_databases": f.allowed_list() },
        "ssas": {
            "server": server, "database": database,
            "user": if f.explicit_auth { f.user.trim() } else { "" },
            "password": if f.explicit_auth { f.password.as_str() } else { "" }
        },
        "adomd": { "dll_path": f.dll.trim() },
        "query": {
            "timeout_seconds": f.timeout.trim().parse::<u32>().unwrap_or(120),
            "max_rows": f.max_rows.trim().parse::<u32>().unwrap_or(50000)
        },
        "log": { "level": "INFO" }
    });
    let cfg_path = dir.join("config.json");
    if ctx.dry_run {
        info(&format!("(simulazione) copierei ssas_proxy.py e scriverei {}", cfg_path.display()));
    } else {
        std::fs::create_dir_all(&dir)?;
        std::fs::copy(src.join("ssas_proxy.py"), dir.join("ssas_proxy.py"))?;
        ok(&format!("copiato ssas_proxy.py in {}", dir.display()));
        write_file(&cfg_path, &serde_json::to_string_pretty(&cfg)?, true)?;
    }

    step("Prerequisiti (Python 3.12)");
    if !run_ps(ctx, &src.join("setup-prerequisites.ps1"), &[])? {
        return err("Installazione dei prerequisiti non riuscita (vedi la finestra PowerShell)");
    }

    step(if f.as_service { "Registrazione del servizio" } else { "Registrazione dell'attività pianificata" });
    let script = clean_path(&dir.join("ssas_proxy.py"));
    let ok_ = if f.as_service {
        let p: Vec<(&str, Value)> = vec![
            ("Port", json!(port)),
            ("ServiceName", json!(svc)),
            ("SsasServer", json!(server)),
            ("SsasDb", json!(database)),
            ("ScriptPath", json!(script)),
        ];
        run_ps(ctx, &src.join("install-service.ps1"), &p)?
    } else {
        let p: Vec<(&str, Value)> = vec![("Port", json!(port)), ("ServiceName", json!(svc)), ("ScriptPath", json!(script))];
        run_ps(ctx, &src.join("install-proxy.ps1"), &p)?
    };
    if !ok_ {
        return err("Registrazione del proxy non riuscita (vedi la finestra PowerShell)");
    }
    if ctx.dry_run {
        ok("Simulazione completata: nessun comando eseguito.");
        return Ok(None);
    }

    step("Verifica");
    if wait_http("localhost", port, "/health", 15) {
        ok(&format!("Il proxy risponde su http://localhost:{port}/health"));
    } else {
        warn("Il proxy non risponde ancora. Controlla i log e la DLL ADOMD.NET.");
    }
    let host = local_ip().unwrap_or_else(|| "IP-DEL-SERVER".into());
    let url = format!("http://{host}:{port}");
    ok("Proxy SSAS installato");
    info(&format!("URL per il backend (SSAS_PROXY_URL): {url}"));
    if !token.is_empty() {
        info(&format!("Token condiviso (SSAS_PROXY_TOKEN): {token}"));
        warn("Conserva il token: il backend Docker/Kubernetes deve usare lo stesso valore (viene precompilato nei form).");
    }
    info(&format!("Configurazione: {}", cfg_path.display()));
    Ok(Some((url, token)))
}
