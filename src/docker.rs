//! Installazione con Docker Compose (app + PostgreSQL).

use crate::common::*;
use crate::gui::*;
use crate::ui::*;
use eframe::egui::Ui;
use std::time::Duration;

#[derive(Clone)]
pub struct DockerForm {
    pub build: bool,
    pub image: String,
    pub pg_bundled: bool,
    pub pg_user: String,
    pub pg_db: String,
    pub pg_pass: String,
    pub db_url: String,
    pub ssas: SsasForm,
    pub llm: LlmForm,
    pub app: AppForm,
}

impl DockerForm {
    pub fn new(has_root: bool) -> Self {
        Self {
            build: has_root,
            image: if has_root { "ssas-ai:latest".into() } else { "redhombre/ssas-ai:latest".into() },
            pg_bundled: true,
            pg_user: "ssas_ai".into(),
            pg_db: "ssas_ai".into(),
            pg_pass: String::new(),
            db_url: String::new(),
            ssas: SsasForm::new(),
            llm: LlmForm::new("http://host.docker.internal:11434"),
            app: AppForm::new(8000),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        section(ui, "Immagine dell'applicazione", |ui| {
            grid(ui, "d-img", |ui| {
                let mut idx = if self.build { 0 } else { 1 };
                combo(ui, "Origine", &mut idx, &["Costruirla dai sorgenti del progetto (docker build)", "Scaricarla da un registry"]);
                if (idx == 0) != self.build {
                    self.build = idx == 0;
                    self.image = if self.build { "ssas-ai:latest".into() } else { "redhombre/ssas-ai:latest".into() };
                }
                text(ui, if self.build { "Nome dell'immagine da creare" } else { "Immagine da scaricare" }, &mut self.image, "registro/nome:tag");
            });
            if !self.build {
                note(ui, "Registry privato? Esegui prima 'docker login <registry>'.");
            }
        });
        section(ui, "Database PostgreSQL", |ui| {
            grid(ui, "d-db", |ui| {
                let mut idx = if self.pg_bundled { 0 } else { 1 };
                combo(ui, "Database", &mut idx, &["Container PostgreSQL incluso (consigliato)", "PostgreSQL esistente (DATABASE_URL)"]);
                self.pg_bundled = idx == 0;
                if self.pg_bundled {
                    text(ui, "Utente", &mut self.pg_user, "");
                    text(ui, "Nome database", &mut self.pg_db, "");
                    secret(ui, "Password", &mut self.pg_pass, "vuoto = generata automaticamente");
                } else {
                    text(ui, "DATABASE_URL", &mut self.db_url, "postgresql://utente:password@host:5432/db");
                }
            });
        });
        self.ssas.ui(ui);
        self.llm.ui(ui, true);
        self.app.ui(ui, Some("Porta HTTP sull'host"));
    }

    pub fn validate(&self, ctx: &Ctx) -> Result<(), String> {
        req("Immagine", &self.image)?;
        if self.build {
            let r = root_of(ctx, "serve per la build")?;
            if !r.join("installazione").join("docker").join("Dockerfile").is_file() {
                return Err("Dockerfile non trovato in installazione/docker".into());
            }
        }
        if self.pg_bundled {
            valid_db_ident("Utente PostgreSQL", &self.pg_user)?;
            valid_db_ident("Nome database", &self.pg_db)?;
        } else {
            valid_pg_url("DATABASE_URL", &self.db_url)?;
        }
        self.ssas.validate()?;
        self.llm.validate()?;
        self.app.validate(true)
    }
}

pub fn run_install(ctx: &Ctx, f: &DockerForm) -> Res<()> {
    step("Verifica Docker");
    let docker_ok = have("docker", &["--version"]);
    let daemon_ok = docker_ok && have("docker", &["info"]);
    let compose_ok = docker_ok && have("docker", &["compose", "version"]);
    if docker_ok && daemon_ok && compose_ok {
        ok("Docker, daemon e Docker Compose v2 disponibili");
    } else {
        let msg = if !docker_ok {
            "Docker non trovato. Installa Docker Desktop / Docker Engine: https://docs.docker.com/get-docker/"
        } else if !daemon_ok {
            "Il daemon Docker non è in esecuzione: avvia Docker Desktop e riprova."
        } else {
            "Docker Compose v2 non trovato: aggiorna Docker o installa il plugin compose."
        };
        if ctx.dry_run {
            warn(&format!("{msg} (ignorato: simulazione)"));
        } else {
            return err(msg);
        }
    }

    let out = ctx.out_base.join("ssas-ai-docker");
    let context = if f.build { Some(clean_path(&root_of(ctx, "build")?).replace('\\', "/")) } else { None };
    let password = if f.pg_pass.is_empty() { rand_alnum(24) } else { f.pg_pass.clone() };

    step("Scrittura dei file");
    let mut env = EnvFile::default();
    env.comment("Generato da ssas-installer. Contiene segreti: non committare.");
    env.blank();
    env.set("HOST_PORT", &f.app.port_u16().to_string());
    if f.pg_bundled {
        env.set("POSTGRES_USER", &f.pg_user);
        env.set("POSTGRES_PASSWORD", &password);
        env.set("POSTGRES_DB", &f.pg_db);
        env.set(
            "DATABASE_URL",
            &format!("postgresql://{}:{}@postgres:5432/{}", url_encode(&f.pg_user), url_encode(&password), f.pg_db),
        );
    } else {
        env.set("DATABASE_URL", &f.db_url);
    }
    env.set("AUTH_SECRET_KEY", &f.app.auth_secret);
    env.blank();
    env.extend(&f.ssas.env_public());
    env.extend(&f.ssas.env_secret());
    env.blank();
    env.extend(&f.llm.env());
    env.blank();
    env.extend(&f.app.env_public());
    env.set("FORWARDED_ALLOW_IPS", "*");
    write_file(&out.join(".env"), &env.render(), true)?;
    write_file(&out.join("docker-compose.yml"), &compose(&f.image, context.as_deref(), f.pg_bundled), false)?;
    write_file(&out.join(".gitignore"), ".env\n*.bak\n", false)?;

    step("Avvio");
    if f.build {
        info("Build dell'immagine (la prima volta richiede alcuni minuti)...");
        if !run_in(ctx, &out, "docker", &["compose", "build"])? {
            return err("Build dell'immagine non riuscita");
        }
    } else if !run_in(ctx, &out, "docker", &["compose", "pull"])? {
        return err("Download dell'immagine non riuscito (docker login necessario?)");
    }
    if !run_in(ctx, &out, "docker", &["compose", "up", "-d"])? {
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
        ok("Container healthy");
    } else {
        warn("Il container non è diventato healthy: controlla i log");
    }
    show_bootstrap(&capture_any("docker", &["logs", "--tail", "300", "ssas-ai"]));

    let f2 = out.join("docker-compose.yml");
    ok(&format!("SSAS AI è in esecuzione su http://localhost:{}", f.app.port_u16()));
    info(&format!("Log   : docker compose -f \"{}\" logs -f", f2.display()));
    info(&format!("Stop  : docker compose -f \"{}\" down", f2.display()));
    Ok(())
}

fn compose(image: &str, build_ctx: Option<&str>, bundled_pg: bool) -> String {
    let mut y = String::from(
        "# Generato da ssas-installer.\nname: ssas-ai\n\nservices:\n",
    );
    if bundled_pg {
        y.push_str(
            r#"  postgres:
    image: postgres:18-alpine
    container_name: ssas-ai-postgres
    restart: unless-stopped
    environment:
      POSTGRES_USER: "${POSTGRES_USER}"
      POSTGRES_PASSWORD: "${POSTGRES_PASSWORD}"
      POSTGRES_DB: "${POSTGRES_DB}"
    volumes:
      - ssas_ai_pgdata:/var/lib/postgresql
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U $${POSTGRES_USER} -d $${POSTGRES_DB}"]
      interval: 5s
      timeout: 5s
      retries: 10
      start_period: 10s
    logging:
      driver: json-file
      options: { max-size: "10m", max-file: "3" }

"#,
        );
    }
    // L'app gira con uid 1001: i volumi nuovi sono di root, quindi un servizio
    // one-shot imposta i permessi prima dell'avvio.
    y.push_str(
        r#"  volume-perms:
    image: busybox:1.36
    user: "0:0"
    restart: "no"
    command: ["sh", "-c", "mkdir -p /app/data /app/logs && chown -R 1001:0 /app/data /app/logs"]
    volumes:
      - ssas_ai_data:/app/data
      - ssas_ai_logs:/app/logs

  ssas-ai:
"#,
    );
    match build_ctx {
        Some(ctx) => y.push_str(&format!(
            "    build:\n      context: \"{ctx}\"\n      dockerfile: installazione/docker/Dockerfile\n    image: {image}\n"
        )),
        None => y.push_str(&format!("    image: {image}\n    pull_policy: always\n")),
    }
    y.push_str(
        r#"    container_name: ssas-ai
    restart: unless-stopped
    depends_on:
      volume-perms:
        condition: service_completed_successfully
"#,
    );
    if bundled_pg {
        y.push_str("      postgres:\n        condition: service_healthy\n");
    }
    y.push_str(
        r#"    ports:
      - "${HOST_PORT:-8000}:8000"
    env_file: .env
    environment:
      PORT: "8000"
      LOG_FILE: /app/logs/ssas-ai.log
    extra_hosts:
      - "host.docker.internal:host-gateway"
    volumes:
      - ssas_ai_data:/app/data
      - ssas_ai_logs:/app/logs
    healthcheck:
      test: ["CMD", "python3", "-c", "import urllib.request; urllib.request.urlopen('http://localhost:8000/healthz')"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 20s
    logging:
      driver: json-file
      options: { max-size: "10m", max-file: "3" }

volumes:
  ssas_ai_data:
  ssas_ai_logs:
"#,
    );
    if bundled_pg {
        y.push_str("  ssas_ai_pgdata:\n");
    }
    y
}

