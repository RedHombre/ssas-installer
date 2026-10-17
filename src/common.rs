//! Contesto condiviso, utilità (file, comandi, segreti) e form comuni
//! (LLM, SSAS, applicazione).

use crate::gui::*;
use crate::ui::*;
use eframe::egui::Ui;
use rand::Rng;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Stato condiviso tra le modalità (clonato nel thread di installazione).
#[derive(Clone)]
pub struct Ctx {
    pub root: Option<PathBuf>,
    pub dry_run: bool,
    pub out_base: PathBuf,
}

// ───────────────────────── segreti ─────────────────────────

pub fn rand_hex(bytes: usize) -> String {
    let mut r = rand::thread_rng();
    (0..bytes).map(|_| format!("{:02x}", r.gen::<u8>())).collect()
}

pub fn rand_alnum(len: usize) -> String {
    const C: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut r = rand::thread_rng();
    (0..len).map(|_| C[r.gen_range(0..C.len())] as char).collect()
}

pub fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ───────────────────────── file ─────────────────────────

/// Insieme ordinato di variabili con commenti, scrivibile come file `.env`.
#[derive(Default)]
pub struct EnvFile {
    lines: Vec<String>,
}

impl EnvFile {
    pub fn comment(&mut self, c: &str) {
        self.lines.push(format!("# {c}"));
    }
    pub fn blank(&mut self) {
        self.lines.push(String::new());
    }
    pub fn set(&mut self, k: &str, v: &str) {
        self.lines.push(format!("{k}={}", env_quote(v)));
    }
    pub fn extend(&mut self, vars: &[(String, String)]) {
        for (k, v) in vars {
            self.set(k, v);
        }
    }
    pub fn render(&self) -> String {
        self.lines.join("\n") + "\n"
    }
}

/// Quoting compatibile con docker compose (`$` e `#` sono speciali).
pub fn env_quote(v: &str) -> String {
    if v.chars().all(|c| c.is_ascii_alphanumeric() || "_./:@%+=,-".contains(c)) {
        v.to_string()
    } else if !v.contains('\'') {
        format!("'{v}'")
    } else {
        let e = v.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$");
        format!("\"{e}\"")
    }
}

pub fn write_file(path: &Path, content: &str, secret: bool) -> Res<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    if path.exists() {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let bak = path.with_file_name(format!("{name}.bak"));
        std::fs::copy(path, &bak)?;
        info(&format!("copia di sicurezza: {}", bak.display()));
    }
    std::fs::write(path, content)?;
    if secret {
        restrict(path);
    }
    ok(&format!("scritto {}", path.display()));
    Ok(())
}

/// Limita i permessi del file al solo utente corrente (best effort).
fn restrict(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(windows)]
    {
        if let Ok(user) = std::env::var("USERNAME") {
            let _ = mk("icacls", &[&path.to_string_lossy(), "/inheritance:r", "/grant:r", &format!("{user}:F"), "Administrators:F", "SYSTEM:F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

// ───────────────────────── comandi ─────────────────────────

fn mk(cmd: &str, args: &[&str]) -> Command {
    let mut c = Command::new(cmd);
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

fn pump<R: Read>(mut r: R) {
    let (mut line, mut buf) = (Vec::new(), [0u8; 4096]);
    let flush = |l: &mut Vec<u8>| {
        if !l.is_empty() {
            out(String::from_utf8_lossy(l).trim_end());
            l.clear();
        }
    };
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                for &b in &buf[..n] {
                    if b == b'\n' || b == b'\r' {
                        flush(&mut line);
                    } else {
                        line.push(b);
                    }
                }
            }
        }
    }
    flush(&mut line);
}

/// Esegue un comando mostrandone l'output nel log. In dry-run lo mostra soltanto.
pub fn run(ctx: &Ctx, cmd: &str, args: &[&str]) -> Res<bool> {
    run_in(ctx, Path::new("."), cmd, args)
}

pub fn run_in(ctx: &Ctx, dir: &Path, cmd: &str, args: &[&str]) -> Res<bool> {
    cmd_line(&format!("{cmd} {}", args.join(" ")));
    if ctx.dry_run {
        return Ok(true);
    }
    let mut c = mk(cmd, args);
    c.current_dir(dir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut ch = c.spawn().map_err(|e| format!("impossibile eseguire '{cmd}': {e}"))?;
    let (o, e) = (ch.stdout.take().unwrap(), ch.stderr.take().unwrap());
    let (h1, h2) = (std::thread::spawn(move || pump(o)), std::thread::spawn(move || pump(e)));
    let st = ch.wait()?;
    let _ = (h1.join(), h2.join());
    Ok(st.success())
}

/// Esegue un comando scrivendo il suo stdout in un file (es. `pg_dump`); stderr va nel log.
pub fn run_to_file(ctx: &Ctx, cmd: &str, args: &[&str], file: &Path) -> Res<bool> {
    cmd_line(&format!("{cmd} {} > {}", args.join(" "), file.display()));
    if ctx.dry_run {
        return Ok(true);
    }
    let f = std::fs::File::create(file)?;
    let mut c = mk(cmd, args);
    c.stdin(Stdio::null()).stdout(f).stderr(Stdio::piped());
    let mut ch = c.spawn().map_err(|e| format!("impossibile eseguire '{cmd}': {e}"))?;
    let e = ch.stderr.take().unwrap();
    let h = std::thread::spawn(move || pump(e));
    let st = ch.wait()?;
    let _ = h.join();
    Ok(st.success())
}

/// Esegue un comando e ne restituisce stdout+stderr se è riuscito.
pub fn capture(cmd: &str, args: &[&str]) -> Option<String> {
    let o = mk(cmd, args).stdin(Stdio::null()).output().ok()?;
    if !o.status.success() {
        return None;
    }
    Some(format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
}

/// Come `capture` ma restituisce l'output anche se il comando è fallito.
pub fn capture_any(cmd: &str, args: &[&str]) -> String {
    match mk(cmd, args).stdin(Stdio::null()).output() {
        Ok(o) => format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
        Err(_) => String::new(),
    }
}

pub fn have(cmd: &str, ver_args: &[&str]) -> bool {
    capture(cmd, ver_args).is_some()
}

/// Mostra le righe del log che contengono le credenziali iniziali dell'amministratore.
pub fn show_bootstrap(log: &str) {
    let lines: Vec<&str> = log
        .lines()
        .filter(|l| {
            let l = l.to_lowercase();
            l.contains("password") || l.contains("bootstrap") || (l.contains("admin") && l.contains("creat"))
        })
        .take(6)
        .collect();
    if lines.is_empty() {
        info("Credenziali iniziali non trovate nel log (admin già creato, oppure app ancora in avvio).");
    } else {
        step("Credenziali iniziali (dal log dell'applicazione)");
        for l in lines {
            ok(l.trim());
        }
        warn("Cambia la password dell'utente 'admin' dopo il primo accesso.");
    }
}

// ───────────────────────── progetto ─────────────────────────

pub fn is_project_root(p: &Path) -> bool {
    p.join("backend").join("main.py").is_file() && p.join("frontend").is_dir()
}

/// Cerca la cartella del progetto risalendo da cwd e dalla posizione dell'eseguibile.
pub fn find_root() -> Option<PathBuf> {
    let starts = [
        std::env::current_dir().ok(),
        std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())),
    ];
    for s in starts.into_iter().flatten() {
        let mut p: Option<&Path> = Some(&s);
        while let Some(d) = p {
            if is_project_root(d) {
                return Some(d.canonicalize().unwrap_or_else(|_| d.to_path_buf()));
            }
            p = d.parent();
        }
    }
    None
}

/// Percorso utilizzabile in YAML/PowerShell: senza il prefisso `\\?\` di canonicalize.
pub fn clean_path(p: &Path) -> String {
    p.to_string_lossy().trim_start_matches(r"\\?\").to_string()
}

pub fn root_of(ctx: &Ctx, why: &str) -> Result<PathBuf, String> {
    ctx.root.clone().ok_or_else(|| {
        format!("Cartella del progetto SSAS AI non valida ({why}): indicala nella barra laterale (deve contenere backend e frontend)")
    })
}

// ───────────────────────── validatori ─────────────────────────

pub fn valid_host(l: &str, h: &str) -> Result<(), String> {
    req(l, h)?;
    if h.contains("://") || h.contains('/') || h.contains(' ') {
        Err(format!("{l}: indica solo il nome host (es. ssas-ai.miodominio.it), senza https:// né percorsi"))
    } else {
        Ok(())
    }
}

pub fn valid_url(l: &str, u: &str) -> Result<(), String> {
    if u.starts_with("http://") || u.starts_with("https://") {
        Ok(())
    } else {
        Err(format!("{l}: l'URL deve iniziare con http:// o https://"))
    }
}

pub fn valid_pg_url(l: &str, u: &str) -> Result<(), String> {
    if u.starts_with("postgresql://") || u.starts_with("postgres://") {
        Ok(())
    } else {
        Err(format!("{l}: deve iniziare con postgresql://"))
    }
}

pub fn valid_k8s_name(l: &str, n: &str) -> Result<(), String> {
    let ok = !n.is_empty()
        && n.len() <= 63
        && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !n.starts_with('-')
        && !n.ends_with('-');
    if ok { Ok(()) } else { Err(format!("{l}: solo minuscole, cifre e '-' (max 63 caratteri)")) }
}

pub fn valid_db_ident(l: &str, n: &str) -> Result<(), String> {
    let first_ok = n.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c == '_');
    let ok = n.len() <= 63 && first_ok && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok { Ok(()) } else { Err(format!("{l}: solo minuscole, cifre e '_' (deve iniziare con una lettera)")) }
}

pub fn valid_dns_name(l: &str, n: &str) -> Result<(), String> {
    let ok = !n.is_empty()
        && n.split('.').all(|p| {
            !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && !p.starts_with('-') && !p.ends_with('-')
        });
    if ok { Ok(()) } else { Err(format!("{l}: nome DNS non valido")) }
}

pub fn valid_svc_name(l: &str, n: &str) -> Result<(), String> {
    if !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        Ok(())
    } else {
        Err(format!("{l}: solo lettere, cifre, '-' e '_'"))
    }
}

// ───────────────────────── form LLM ─────────────────────────

const PROVIDERS: &[(&str, &str, Option<&str>, &str)] = &[
    ("OpenRouter (consigliato: un'unica chiave, molti modelli)", "openrouter", Some("OPENROUTER_API_KEY"), "anthropic/claude-sonnet-4.6"),
    ("Anthropic Claude (diretto)", "claude", Some("ANTHROPIC_API_KEY"), "claude-sonnet-4-5"),
    ("OpenAI", "openai", Some("OPENAI_API_KEY"), "gpt-4o"),
    ("Google Gemini", "gemini", Some("GEMINI_API_KEY"), "gemini-2.0-flash"),
    ("Ollama (locale, nessuna chiave)", "ollama", None, "llama3.2"),
];

#[derive(Clone)]
pub struct LlmForm {
    pub provider: usize,
    prev: usize,
    pub key: String,
    pub model: String,
    pub ollama_url: String,
}

impl LlmForm {
    pub fn new(ollama_default: &str) -> Self {
        Self { provider: 0, prev: 0, key: String::new(), model: PROVIDERS[0].3.into(), ollama_url: ollama_default.into() }
    }

    pub fn ui(&mut self, ui: &mut Ui, with_ollama: bool) {
        section(ui, "Provider LLM", |ui| {
            grid(ui, "llm", |ui| {
                let n = if with_ollama { PROVIDERS.len() } else { PROVIDERS.len() - 1 };
                let labels: Vec<&str> = PROVIDERS[..n].iter().map(|p| p.0).collect();
                combo(ui, "Provider", &mut self.provider, &labels);
                if self.provider != self.prev {
                    self.model = PROVIDERS[self.provider].3.into();
                    self.prev = self.provider;
                }
                match PROVIDERS[self.provider].2 {
                    Some(kv) => secret(ui, &format!("Chiave API ({kv})"), &mut self.key, "vuoto = la imposti dopo dall'app"),
                    None => text(ui, "URL server Ollama", &mut self.ollama_url, "http://host.docker.internal:11434"),
                }
                text(ui, "Modello predefinito", &mut self.model, "");
            });
        });
    }

    pub fn name(&self) -> &'static str {
        PROVIDERS[self.provider].1
    }
    fn key_var(&self) -> Option<&'static str> {
        PROVIDERS[self.provider].2
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.key_var().is_none() {
            valid_url("URL Ollama", &self.ollama_url)?;
        }
        req("Modello LLM", &self.model)
    }

    pub fn env(&self) -> Vec<(String, String)> {
        let mut v = vec![("LLM_PROVIDER".to_string(), self.name().to_string())];
        match self.key_var() {
            Some(kv) if !self.key.is_empty() => v.push((kv.to_string(), self.key.clone())),
            Some(_) => {}
            None => v.push(("OLLAMA_BASE_URL".into(), self.ollama_url.clone())),
        }
        v.push((format!("LLM_MODEL_{}", self.name().to_uppercase()), self.model.clone()));
        v
    }
    pub fn env_public(&self) -> Vec<(String, String)> {
        self.env().into_iter().filter(|(k, _)| !k.ends_with("_API_KEY")).collect()
    }
    pub fn env_secret(&self) -> Vec<(String, String)> {
        self.env().into_iter().filter(|(k, _)| k.ends_with("_API_KEY")).collect()
    }
}

// ───────────────────────── form SSAS (Docker/Kubernetes) ─────────────────────────

#[derive(Clone)]
pub struct SsasForm {
    /// 0 proxy, 1 XMLA, 2 demo
    pub mode: usize,
    pub proxy_url: String,
    pub proxy_token: String,
    pub xmla_url: String,
    pub user: String,
    pub password: String,
    pub server: String,
    pub database: String,
}

impl SsasForm {
    pub fn new() -> Self {
        Self {
            mode: 2,
            proxy_url: String::new(),
            proxy_token: String::new(),
            xmla_url: String::new(),
            user: String::new(),
            password: String::new(),
            server: String::new(),
            database: String::new(),
        }
    }

    /// Precompila con il proxy appena installato.
    pub fn apply_proxy(&mut self, url: &str, token: &str) {
        self.mode = 0;
        self.proxy_url = url.into();
        self.proxy_token = token.into();
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        section(ui, "Connessione a SSAS", |ui| {
            grid(ui, "ssas", |ui| {
                combo(
                    ui,
                    "Metodo",
                    &mut self.mode,
                    &[
                        "Proxy REST su Windows (ssas_proxy.py) — consigliato",
                        "XMLA over HTTP (IIS msmdpump.dll)",
                        "DEMO (dati simulati, nessun SSAS)",
                    ],
                );
                match self.mode {
                    0 => {
                        text(ui, "URL del proxy", &mut self.proxy_url, "http://192.168.1.50:9000 (non usare localhost)");
                        secret(ui, "Token del proxy", &mut self.proxy_token, "SSAS_PROXY_TOKEN, vuoto = nessuno");
                    }
                    1 => {
                        text(ui, "URL XMLA", &mut self.xmla_url, "http://192.168.1.50/olap/msmdpump.dll");
                        text(ui, "Utente SSAS", &mut self.user, "opzionale");
                        secret(ui, "Password SSAS", &mut self.password, "");
                    }
                    _ => {}
                }
                if self.mode != 2 {
                    text(ui, "Server SSAS (informativo)", &mut self.server, "nome del server, per il catalogo 'Default'");
                    text(ui, "Database OLAP/Tabular", &mut self.database, "");
                }
            });
        });
    }

    pub fn validate(&self) -> Result<(), String> {
        match self.mode {
            0 => valid_url("URL del proxy", &self.proxy_url)?,
            1 => valid_url("URL XMLA", &self.xmla_url)?,
            _ => return Ok(()),
        }
        req("Database SSAS", &self.database)
    }

    pub fn demo(&self) -> bool {
        self.mode == 2
    }

    pub fn env_public(&self) -> Vec<(String, String)> {
        let mut v = vec![("SSAS_DEMO_MODE".to_string(), self.demo().to_string())];
        if !self.demo() {
            v.push(("SSAS_SERVER".into(), self.server.clone()));
            v.push(("SSAS_DATABASE".into(), self.database.clone()));
            v.push(("SSAS_PROXY_URL".into(), if self.mode == 0 { self.proxy_url.clone() } else { String::new() }));
            v.push(("SSAS_XMLA_URL".into(), if self.mode == 1 { self.xmla_url.clone() } else { String::new() }));
        }
        v
    }
    pub fn env_secret(&self) -> Vec<(String, String)> {
        let mut v = vec![];
        if self.mode == 0 && !self.proxy_token.is_empty() {
            v.push(("SSAS_PROXY_TOKEN".to_string(), self.proxy_token.clone()));
        }
        if self.mode == 1 && !self.user.is_empty() {
            v.push(("SSAS_USER".into(), self.user.clone()));
            v.push(("SSAS_PASSWORD".into(), self.password.clone()));
        }
        v
    }
}

// ───────────────────────── form applicazione ─────────────────────────

#[derive(Clone)]
pub struct AppForm {
    pub port: String,
    pub advanced: bool,
    pub timeout: String,
    pub max_rows: String,
    pub cors: String,
    pub token_hours: String,
    pub auth_secret: String,
    pub webauthn: bool,
    pub webauthn_host: String,
}

impl AppForm {
    pub fn new(default_port: u16) -> Self {
        Self {
            port: default_port.to_string(),
            advanced: false,
            timeout: "30".into(),
            max_rows: "10000".into(),
            cors: "*".into(),
            token_hours: "8".into(),
            auth_secret: rand_hex(32),
            webauthn: false,
            webauthn_host: String::new(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, port_label: Option<&str>) {
        section(ui, "Applicazione", |ui| {
            grid(ui, "app", |ui| {
                if let Some(l) = port_label {
                    text(ui, l, &mut self.port, "8000");
                }
                ui.label("Chiave di firma (AUTH_SECRET_KEY)");
                ui.horizontal(|ui| {
                    ui.add(eframe::egui::TextEdit::singleline(&mut self.auth_secret).password(true).desired_width(360.0));
                    if ui.button("Rigenera").clicked() {
                        self.auth_secret = rand_hex(32);
                    }
                });
                ui.end_row();
                check(ui, "Passkey / WebAuthn", &mut self.webauthn, "accesso con impronta / Windows Hello (richiede HTTPS e dominio)");
                if self.webauthn {
                    text(ui, "Dominio (WEBAUTHN_RP_ID)", &mut self.webauthn_host, "ssas-ai.miodominio.it");
                }
                check(ui, "Impostazioni avanzate", &mut self.advanced, "timeout, limiti, CORS, durata sessione");
                if self.advanced {
                    text(ui, "Timeout query SSAS (s)", &mut self.timeout, "");
                    text(ui, "Righe massime per risultato", &mut self.max_rows, "");
                    text(ui, "Origini CORS", &mut self.cors, "* = tutte");
                    text(ui, "Durata sessione (ore)", &mut self.token_hours, "");
                }
            });
        });
    }

    pub fn validate(&self, check_port: bool) -> Result<(), String> {
        if check_port {
            num::<u16>("Porta", &self.port, 1, 65535)?;
        }
        num::<u32>("Timeout query", &self.timeout, 1, 3600)?;
        num::<u32>("Righe massime", &self.max_rows, 1, 1_000_000)?;
        num::<u32>("Durata sessione", &self.token_hours, 1, 720)?;
        req("Chiave di firma", &self.auth_secret)?;
        if self.webauthn {
            valid_host("Dominio WebAuthn", &self.webauthn_host)?;
        }
        Ok(())
    }

    pub fn port_u16(&self) -> u16 {
        self.port.trim().parse().unwrap_or(8000)
    }

    pub fn env_public(&self) -> Vec<(String, String)> {
        let mut v = vec![
            ("QUERY_TIMEOUT_SECONDS".to_string(), self.timeout.trim().to_string()),
            ("MAX_RESULT_ROWS".into(), self.max_rows.trim().to_string()),
            ("CORS_ORIGINS".into(), self.cors.clone()),
            ("AUTH_ENABLED".into(), "true".into()),
            ("AUTH_TOKEN_EXPIRE_HOURS".into(), self.token_hours.trim().to_string()),
        ];
        if self.webauthn {
            let h = self.webauthn_host.trim();
            v.push(("WEBAUTHN_ENABLED".into(), "true".into()));
            v.push(("WEBAUTHN_RP_ID".into(), h.into()));
            v.push(("WEBAUTHN_RP_NAME".into(), "SSAS AI".into()));
            v.push(("WEBAUTHN_ORIGIN".into(), format!("https://{h}")));
        } else {
            v.push(("WEBAUTHN_ENABLED".into(), "false".into()));
        }
        v
    }
}

// ───────────────────────── Windows / rete ─────────────────────────

pub fn is_admin() -> bool {
    cfg!(windows) && have("net", &["session"])
}

/// Riavvia il programma con i privilegi di Amministratore (UAC).
pub fn relaunch_as_admin() -> Res<()> {
    let exe = std::env::current_exe()?;
    let cmd = format!("Start-Process -FilePath '{}' -Verb RunAs", exe.display().to_string().replace('\'', "''"));
    mk("powershell.exe", &["-NoProfile", "-Command", &cmd]).spawn()?;
    Ok(())
}

/// Esegue uno script PowerShell del progetto in una finestra di console visibile
/// (gli script possono chiedere password o aprire finestre di dialogo).
/// `params` sono passati come parametri con nome; `true` attiva uno switch.
pub fn run_ps(ctx: &Ctx, script: &Path, params: &[(&str, serde_json::Value)]) -> Res<bool> {
    let summary: Vec<String> = params
        .iter()
        .map(|(k, v)| match v {
            serde_json::Value::Bool(_) => format!("-{k}"),
            _ if k.to_lowercase().contains("url") || k.to_lowercase().contains("password") => format!("-{k} ***"),
            _ => format!("-{k} {v}"),
        })
        .collect();
    cmd_line(&format!("powershell {} {}", clean_path(script), summary.join(" ")));
    if ctx.dry_run {
        return Ok(true);
    }
    run_ps_console(script, params)
}

#[cfg(not(windows))]
fn run_ps_console(_script: &Path, _params: &[(&str, serde_json::Value)]) -> Res<bool> {
    err("Gli script PowerShell si eseguono solo su Windows")
}

#[cfg(windows)]
fn run_ps_console(script: &Path, params: &[(&str, serde_json::Value)]) -> Res<bool> {
    use std::os::windows::process::CommandExt;
    let tmp = std::env::temp_dir().join(format!("ssas-installer-{}", rand_alnum(8)));
    std::fs::create_dir_all(&tmp)?;
    let wrapper = tmp.join("run.ps1");
    let json = tmp.join("params.json");
    let result = tmp.join("result.txt");
    let transcript = tmp.join("transcript.txt");
    std::fs::write(
        &wrapper,
        "param([string]$Script,[string]$Json,[string]$Result,[string]$Transcript)\r\n\
         $code = 0\r\n\
         try { Start-Transcript -Path $Transcript -Force | Out-Null } catch { }\r\n\
         try {\r\n\
           $o = Get-Content -Raw -Encoding UTF8 $Json | ConvertFrom-Json\r\n\
           $h = @{}\r\n\
           foreach ($p in $o.PSObject.Properties) { $h[$p.Name] = $p.Value }\r\n\
           & $Script @h\r\n\
         } catch { Write-Host \"[FAIL] $_\" -ForegroundColor Red; $code = 1 }\r\n\
         try { Stop-Transcript | Out-Null } catch { }\r\n\
         Set-Content -Path $Result -Value $code\r\n\
         Write-Host ''\r\n\
         Read-Host 'Premi INVIO per chiudere questa finestra'\r\n",
    )?;
    let map: serde_json::Map<String, serde_json::Value> = params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    std::fs::write(&json, serde_json::to_string(&map)?)?;
    info("Si apre una finestra PowerShell: segui le eventuali richieste e premi INVIO alla fine.");
    let st = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&wrapper)
        .args(["-Script", &clean_path(script), "-Json"])
        .arg(&json)
        .arg("-Result")
        .arg(&result)
        .arg("-Transcript")
        .arg(&transcript)
        .creation_flags(0x0000_0010) // CREATE_NEW_CONSOLE
        .status();
    let code = std::fs::read_to_string(&result).ok().and_then(|s| s.trim().trim_start_matches('\u{feff}').parse::<i32>().ok());
    // Gli script segnalano gli errori con "[FAIL]" ed `exit 1`: l'esito si ricava dal transcript,
    // perché $LASTEXITCODE può contenere il codice di un comando precedente (es. robocopy = 1).
    let bytes = std::fs::read(&transcript).unwrap_or_default();
    let text = if bytes.starts_with(&[0xFF, 0xFE]) {
        let u: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&u)
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let _ = std::fs::remove_dir_all(&tmp);
    st?;
    let failed = code != Some(0) || text.lines().any(|l| l.contains("[FAIL]"));
    if failed {
        warn("Lo script ha segnalato un errore. Ultime righe:");
        let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        for l in &lines[lines.len().saturating_sub(25)..] {
            out(l);
        }
    }
    Ok(!failed)
}

/// GET HTTP minimale (senza dipendenze): true se la risposta è 200.
pub fn http_ok(host: &str, port: u16, path: &str) -> bool {
    use std::io::Write;
    use std::net::{TcpStream, ToSocketAddrs};
    let Some(addr) = (host, port).to_socket_addrs().ok().and_then(|mut a| a.next()) else {
        return false;
    };
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_secs(3)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let req = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = [0u8; 32];
    let n = s.read(&mut buf).unwrap_or(0);
    String::from_utf8_lossy(&buf[..n]).contains(" 200")
}

/// Attende che un endpoint HTTP risponda 200 (fino a `tries` tentativi, 2 s l'uno).
pub fn wait_http(host: &str, port: u16, path: &str, tries: u32) -> bool {
    for _ in 0..tries {
        if http_ok(host, port, path) {
            return true;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    false
}

/// IPv4 locale usato per le connessioni in uscita (nessun pacchetto viene inviato).
pub fn local_ip() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    Some(s.local_addr().ok()?.ip().to_string())
}
