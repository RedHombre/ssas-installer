//! Installazione su Kubernetes: genera i manifest e li applica con kubectl.

use crate::common::*;
use crate::gui::*;
use crate::ui::*;
use eframe::egui::Ui;
use std::path::Path;
use std::time::Duration;

#[derive(Clone)]
pub struct K8sForm {
    pub contexts: Vec<String>,
    pub kctx: usize,
    pub ns: String,
    pub build_push: bool,
    pub image: String,
    pub private_registry: bool,
    pub reg_server: String,
    pub reg_user: String,
    pub reg_pass: String,
    pub replicas: String,
    pub rwx: bool,
    pub storage_class: String,
    pub data_size: String,
    pub logs_size: String,
    pub hpa: bool,
    pub hpa_min: String,
    pub hpa_max: String,
    pub ingress: bool,
    pub ing_class: String,
    pub host: String,
    /// 0 Let's Encrypt (crea issuer), 1 issuer esistente, 2 nessun TLS
    pub tls: usize,
    pub issuer: String,
    pub email: String,
    pub install_infra: bool,
    pub svc_type: usize,
    pub pg_incluster: bool,
    pub pg_user: String,
    pub pg_db: String,
    pub pg_pass: String,
    pub pg_size: String,
    pub db_url: String,
    pub ssas: SsasForm,
    pub llm: LlmForm,
    pub app: AppForm,
}

impl K8sForm {
    pub fn new() -> Self {
        let mut f = Self {
            contexts: vec![],
            kctx: 0,
            ns: "ssas-ai".into(),
            build_push: false,
            image: "redhombre/ssas-ai:latest".into(),
            private_registry: false,
            reg_server: "https://index.docker.io/v1/".into(),
            reg_user: String::new(),
            reg_pass: String::new(),
            replicas: "2".into(),
            rwx: true,
            storage_class: String::new(),
            data_size: "1Gi".into(),
            logs_size: "2Gi".into(),
            hpa: true,
            hpa_min: "2".into(),
            hpa_max: "6".into(),
            ingress: true,
            ing_class: "nginx".into(),
            host: String::new(),
            tls: 0,
            issuer: "letsencrypt-prod".into(),
            email: String::new(),
            install_infra: true,
            svc_type: 0,
            pg_incluster: true,
            pg_user: "ssas_ai".into(),
            pg_db: "ssas_ai".into(),
            pg_pass: String::new(),
            pg_size: "5Gi".into(),
            db_url: String::new(),
            ssas: SsasForm::new(),
            llm: LlmForm::new("http://ollama.default.svc.cluster.local:11434"),
            app: AppForm::new(8000),
        };
        f.refresh_contexts();
        f
    }

    pub fn refresh_contexts(&mut self) {
        let (list, cur) = list_contexts();
        self.contexts = list;
        self.kctx = cur;
    }

    fn context(&self) -> String {
        self.contexts.get(self.kctx).cloned().unwrap_or_else(|| "(nessuno)".into())
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        section(ui, "Cluster e applicazione", |ui| {
            grid(ui, "k-cluster", |ui| {
                ui.label("Cluster (context kubectl)");
                ui.horizontal(|ui| {
                    if self.contexts.is_empty() {
                        ui.label("nessun context trovato (kubectl installato e configurato?)");
                    } else {
                        let sel = self.context();
                        eframe::egui::ComboBox::from_id_salt("kctx").selected_text(sel).width(360.0).show_ui(ui, |ui| {
                            for (i, c) in self.contexts.clone().iter().enumerate() {
                                ui.selectable_value(&mut self.kctx, i, c);
                            }
                        });
                    }
                    if ui.button("Aggiorna").clicked() {
                        self.refresh_contexts();
                    }
                });
                ui.end_row();
                text(ui, "Namespace", &mut self.ns, "ssas-ai");
                let mut idx = usize::from(self.build_push);
                combo(ui, "Immagine", &mut idx, &["Usare un'immagine già presente in un registry", "Costruirla dai sorgenti e pubblicarla (docker build + push)"]);
                self.build_push = idx == 1;
                text(ui, "Nome immagine", &mut self.image, "registro/nome:tag");
                check(ui, "Registry privato", &mut self.private_registry, "crea un imagePullSecret");
                if self.private_registry {
                    text(ui, "Server del registry", &mut self.reg_server, "");
                    text(ui, "Utente", &mut self.reg_user, "");
                    secret(ui, "Password / token", &mut self.reg_pass, "");
                }
            });
            if self.build_push {
                note(ui, "Serve Docker in esecuzione e 'docker login' già fatto sul registry.");
            }
        });
        section(ui, "Repliche e storage", |ui| {
            grid(ui, "k-scale", |ui| {
                text(ui, "Numero di repliche", &mut self.replicas, "");
                check(ui, "Storage condiviso", &mut self.rwx, "ReadWriteMany (necessario con più repliche; es. NFS, CephFS, Azure Files)");
                text(ui, "StorageClass", &mut self.storage_class, "vuoto = predefinita del cluster");
                text(ui, "Volume dati", &mut self.data_size, "1Gi");
                text(ui, "Volume log", &mut self.logs_size, "2Gi");
                if self.rwx {
                    check(ui, "Autoscaling (HPA)", &mut self.hpa, "scala le repliche in base a CPU/RAM");
                    if self.hpa {
                        text(ui, "Repliche minime", &mut self.hpa_min, "");
                        text(ui, "Repliche massime", &mut self.hpa_max, "");
                    }
                } else {
                    ui.label("Con ReadWriteOnce le repliche vengono ridotte a 1.");
                    ui.end_row();
                }
            });
        });
        section(ui, "Esposizione di rete", |ui| {
            grid(ui, "k-net", |ui| {
                check(ui, "Ingress", &mut self.ingress, "dominio + HTTPS");
                if self.ingress {
                    text(ui, "IngressClass", &mut self.ing_class, "nginx");
                    text(ui, "Dominio pubblico", &mut self.host, "ssas-ai.miodominio.it");
                    combo(
                        ui,
                        "HTTPS",
                        &mut self.tls,
                        &["Let's Encrypt automatico (crea un ClusterIssuer)", "ClusterIssuer cert-manager già esistente", "Solo HTTP (nessun TLS)"],
                    );
                    if self.tls < 2 {
                        text(ui, "Nome del ClusterIssuer", &mut self.issuer, "letsencrypt-prod");
                    }
                    if self.tls == 0 {
                        text(ui, "Email per Let's Encrypt", &mut self.email, "");
                    }
                    check(ui, "Componenti mancanti", &mut self.install_infra, "installa ingress-nginx e cert-manager con Helm se non presenti");
                }
                combo(ui, "Tipo di Service", &mut self.svc_type, &["ClusterIP (interno / dietro Ingress)", "LoadBalancer (IP esterno)", "NodePort"]);
            });
        });
        section(ui, "Database PostgreSQL", |ui| {
            grid(ui, "k-db", |ui| {
                let mut idx = usize::from(!self.pg_incluster);
                combo(ui, "Database", &mut idx, &["PostgreSQL dentro il cluster (StatefulSet)", "PostgreSQL esistente (DATABASE_URL)"]);
                self.pg_incluster = idx == 0;
                if self.pg_incluster {
                    text(ui, "Utente", &mut self.pg_user, "");
                    text(ui, "Nome database", &mut self.pg_db, "");
                    secret(ui, "Password", &mut self.pg_pass, "vuoto = generata automaticamente");
                    text(ui, "Dimensione volume", &mut self.pg_size, "5Gi");
                } else {
                    text(ui, "DATABASE_URL", &mut self.db_url, "postgresql://utente:password@host:5432/db");
                }
            });
        });
        self.ssas.ui(ui);
        self.llm.ui(ui, true);
        self.app.ui(ui, None);
    }

    pub fn validate(&self, ctx: &Ctx) -> Result<(), String> {
        if self.contexts.is_empty() && !ctx.dry_run {
            return Err("Nessun context kubectl: installa kubectl e configura l'accesso al cluster".into());
        }
        valid_k8s_name("Namespace", &self.ns)?;
        req("Immagine", &self.image)?;
        if self.build_push {
            root_of(ctx, "serve per la build")?;
        }
        if self.private_registry {
            req("Server del registry", &self.reg_server)?;
            req("Utente del registry", &self.reg_user)?;
            req("Password del registry", &self.reg_pass)?;
        }
        num::<u32>("Repliche", &self.replicas, 1, 20)?;
        req("Volume dati", &self.data_size)?;
        req("Volume log", &self.logs_size)?;
        if self.rwx && self.hpa {
            let min = num::<u32>("Repliche minime", &self.hpa_min, 1, 50)?;
            num::<u32>("Repliche massime", &self.hpa_max, min, 100)?;
        }
        if self.ingress {
            req("IngressClass", &self.ing_class)?;
            valid_dns_name("Dominio", &self.host)?;
            if self.tls < 2 {
                req("ClusterIssuer", &self.issuer)?;
            }
            if self.tls == 0 && !self.email.contains('@') {
                return Err("Email per Let's Encrypt: indirizzo non valido".into());
            }
        }
        if self.pg_incluster {
            valid_db_ident("Utente PostgreSQL", &self.pg_user)?;
            valid_db_ident("Nome database", &self.pg_db)?;
            req("Dimensione volume PostgreSQL", &self.pg_size)?;
        } else {
            valid_pg_url("DATABASE_URL", &self.db_url)?;
        }
        self.ssas.validate()?;
        self.llm.validate()?;
        self.app.validate(false)
    }
}

/// Context kubectl disponibili e indice di quello corrente.
pub fn list_contexts() -> (Vec<String>, usize) {
    let list = capture_any("kubectl", &["config", "get-contexts", "-o", "name"]);
    let cur = capture_any("kubectl", &["config", "current-context"]).trim().to_string();
    let v: Vec<String> = list.lines().map(str::trim).filter(|l| !l.is_empty() && !l.contains(' ')).map(String::from).collect();
    let idx = v.iter().position(|c| *c == cur).unwrap_or(0);
    (v, idx)
}

struct Tls {
    issuer: String,
    /// Email per creare un ClusterIssuer Let's Encrypt; None = si usa un issuer già esistente.
    create_with_email: Option<String>,
}

struct Ingress {
    class: String,
    host: String,
    tls: Option<Tls>,
}

struct PullSecret {
    server: String,
    user: String,
    password: String,
}

/// Stringa tra apici valida in YAML (la sintassi JSON è un sottoinsieme di YAML).
fn q(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn b64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        o.push(T[(n >> 18) as usize & 63] as char);
        o.push(T[(n >> 12) as usize & 63] as char);
        o.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        o.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    o
}

/// kubectl con il context scelto.
pub(crate) fn kc(ctx: &Ctx, kctx: &str, args: &[&str]) -> Res<bool> {
    let mut a = vec!["--context", kctx];
    a.extend_from_slice(args);
    run(ctx, "kubectl", &a)
}

pub(crate) fn kcap(kctx: &str, args: &[&str]) -> String {
    let mut a = vec!["--context", kctx];
    a.extend_from_slice(args);
    capture_any("kubectl", &a)
}

fn ns_exists(kctx: &str, name: &str) -> bool {
    capture("kubectl", &["--context", kctx, "get", "ns", name, "-o", "name"]).is_some()
}

fn kapply(ctx: &Ctx, kctx: &str, file: &Path) -> Res<bool> {
    kc(ctx, kctx, &["apply", "-f", &file.to_string_lossy()])
}

pub fn run_install(ctx: &Ctx, f: &K8sForm) -> Res<()> {
    step("Verifica kubectl e cluster");
    let have_kubectl = have("kubectl", &["version", "--client"]);
    let kctx = if f.contexts.is_empty() { "(dry-run)".to_string() } else { f.context() };
    if !have_kubectl {
        let m = "kubectl non trovato. Installalo: https://kubernetes.io/docs/tasks/tools/ (Windows: winget install Kubernetes.kubectl)";
        if ctx.dry_run {
            warn(&format!("{m} (ignorato: simulazione)"));
        } else {
            return err(m);
        }
    } else if have("kubectl", &["--context", &kctx, "get", "ns"]) {
        ok(&format!("Cluster '{kctx}' raggiungibile"));
    } else if ctx.dry_run {
        warn(&format!("Cluster '{kctx}' non raggiungibile (ignorato: simulazione)"));
    } else {
        return err(format!("Il cluster '{kctx}' non è raggiungibile (kubectl --context {kctx} get ns)"));
    }

    let ns = f.ns.trim().to_string();
    let image = f.image.trim().to_string();
    let rwx = f.rwx;
    let replicas = if rwx { f.replicas.trim().parse::<u32>().unwrap_or(1) } else { 1 };
    let hpa = if rwx && f.hpa {
        Some((f.hpa_min.trim().parse::<u32>().unwrap_or(1), f.hpa_max.trim().parse::<u32>().unwrap_or(3)))
    } else {
        None
    };
    let pull_secret = f.private_registry.then(|| PullSecret {
        server: f.reg_server.trim().into(),
        user: f.reg_user.trim().into(),
        password: f.reg_pass.clone(),
    });
    let ingress = f.ingress.then(|| Ingress {
        class: f.ing_class.trim().into(),
        host: f.host.trim().into(),
        tls: match f.tls {
            0 => Some(Tls { issuer: f.issuer.trim().into(), create_with_email: Some(f.email.trim().into()) }),
            1 => Some(Tls { issuer: f.issuer.trim().into(), create_with_email: None }),
            _ => None,
        },
    });
    let svc_type = ["ClusterIP", "LoadBalancer", "NodePort"][f.svc_type.min(2)];
    let pg_password = if f.pg_pass.is_empty() { rand_alnum(24) } else { f.pg_pass.clone() };
    let out = ctx.out_base.join("ssas-ai-k8s");

    // ── manifest ──
    step("Scrittura dei manifest");
    let database_url = if f.pg_incluster {
        format!("postgresql://{}:{}@ssas-ai-postgres:5432/{}", url_encode(&f.pg_user), url_encode(&pg_password), f.pg_db)
    } else {
        f.db_url.clone()
    };
    let mut secret_vars: Vec<(String, String)> =
        vec![("AUTH_SECRET_KEY".into(), f.app.auth_secret.clone()), ("DATABASE_URL".into(), database_url)];
    secret_vars.extend(f.ssas.env_secret());
    secret_vars.extend(f.llm.env_secret());
    if f.pg_incluster {
        secret_vars.push(("POSTGRES_PASSWORD".into(), pg_password.clone()));
    }
    let mut cm_vars: Vec<(String, String)> = vec![("PORT".into(), "8000".into())];
    cm_vars.extend(f.ssas.env_public());
    cm_vars.extend(f.llm.env_public());
    cm_vars.extend(f.app.env_public());

    let sc = f.storage_class.trim();
    let mut files: Vec<(&str, String, bool)> = vec![];
    files.push(("00-namespace.yaml", namespace_yaml(&ns), false));
    files.push(("01-secret.yaml", secret_yaml(&ns, &secret_vars, pull_secret.as_ref()), true));
    files.push(("02-configmap.yaml", configmap_yaml(&ns, &cm_vars), false));
    if f.pg_incluster {
        files.push(("03-postgres.yaml", postgres_yaml(&ns, &f.pg_user, &f.pg_db, f.pg_size.trim(), sc), false));
    }
    files.push(("04-pvc.yaml", pvc_yaml(&ns, sc, f.data_size.trim(), f.logs_size.trim(), rwx), false));
    files.push(("05-deployment.yaml", deployment_yaml(&ns, &image, replicas, rwx, pull_secret.is_some(), f.pg_incluster), false));
    files.push(("06-service.yaml", service_yaml(&ns, svc_type), false));
    let mut issuer_file: Option<&str> = None;
    if let Some(i) = &ingress {
        if let Some(Tls { issuer, create_with_email: Some(email) }) = &i.tls {
            files.push(("07-clusterissuer.yaml", issuer_yaml(issuer, email, &i.class), false));
            issuer_file = Some("07-clusterissuer.yaml");
        }
        files.push(("08-ingress.yaml", ingress_yaml(&ns, i), false));
    }
    if let Some((min, max)) = hpa {
        files.push(("09-hpa.yaml", hpa_yaml(&ns, min, max), false));
    }
    for (name, content, secret) in &files {
        write_file(&out.join(name), content, *secret)?;
    }
    warn("01-secret.yaml contiene segreti in chiaro: non committarlo nel repository.");

    // ── infrastruttura ──
    if let Some(i) = &ingress {
        if f.install_infra {
            step("Componenti del cluster");
            let has_helm = have("helm", &["version"]);
            if i.class == "nginx" {
                if !ctx.dry_run && ns_exists(&kctx, "ingress-nginx") {
                    ok("ingress-nginx già presente");
                } else if has_helm || ctx.dry_run {
                    run(ctx, "helm", &["repo", "add", "ingress-nginx", "https://kubernetes.github.io/ingress-nginx", "--force-update"])?;
                    if !run(ctx, "helm", &[
                        "upgrade", "--install", "ingress-nginx", "ingress-nginx/ingress-nginx", "--kube-context", &kctx,
                        "--namespace", "ingress-nginx", "--create-namespace", "--wait", "--timeout", "180s",
                    ])? {
                        return err("Installazione di ingress-nginx non riuscita");
                    }
                } else {
                    warn("ingress-nginx non trovato e helm non disponibile: installalo manualmente.");
                }
            }
            if i.tls.is_some() {
                if !ctx.dry_run && ns_exists(&kctx, "cert-manager") {
                    ok("cert-manager già presente");
                } else if has_helm || ctx.dry_run {
                    run(ctx, "helm", &["repo", "add", "jetstack", "https://charts.jetstack.io", "--force-update"])?;
                    if !run(ctx, "helm", &[
                        "upgrade", "--install", "cert-manager", "jetstack/cert-manager", "--kube-context", &kctx,
                        "--namespace", "cert-manager", "--create-namespace", "--set", "crds.enabled=true", "--wait", "--timeout", "180s",
                    ])? {
                        return err("Installazione di cert-manager non riuscita");
                    }
                } else {
                    warn("cert-manager non trovato e helm non disponibile: installalo manualmente.");
                }
            }
        }
    }

    // ── immagine ──
    if f.build_push {
        step("Build e push dell'immagine");
        let rp = clean_path(&root_of(ctx, "build")?);
        let df = format!("{}/installazione/docker/Dockerfile", rp.replace('\\', "/"));
        if !run(ctx, "docker", &["build", "-f", &df, "-t", &image, &rp])? {
            return err("Build dell'immagine non riuscita");
        }
        if !run(ctx, "docker", &["push", &image])? {
            return err("Push dell'immagine non riuscito (docker login sul registry?)");
        }
    }

    // ── applicazione ──
    step("Applicazione dei manifest");
    for (name, _, _) in &files {
        let path = out.join(name);
        let mut done = kapply(ctx, &kctx, &path)?;
        if !done && Some(*name) == issuer_file && !ctx.dry_run {
            // il webhook di cert-manager può impiegare qualche secondo ad attivarsi
            for _ in 0..6 {
                std::thread::sleep(Duration::from_secs(10));
                if kapply(ctx, &kctx, &path)? {
                    done = true;
                    break;
                }
            }
        }
        if !done {
            return err(format!("kubectl apply fallito per {name}"));
        }
    }
    if ctx.dry_run {
        ok("Simulazione completata: nessun comando eseguito.");
        return Ok(());
    }

    step("Attesa del rollout");
    if f.pg_incluster {
        kc(ctx, &kctx, &["-n", &ns, "rollout", "status", "statefulset/ssas-ai-postgres", "--timeout=300s"])?;
    }
    let rolled = kc(ctx, &kctx, &["-n", &ns, "rollout", "status", "deployment/ssas-ai", "--timeout=420s"])?;
    kc(ctx, &kctx, &["-n", &ns, "get", "pods", "-o", "wide"])?;
    if rolled {
        show_bootstrap(&kcap(&kctx, &["-n", &ns, "logs", "deploy/ssas-ai", "--tail=300"]));
    } else {
        warn("Il rollout non si è completato in tempo. Diagnostica:");
        info(&format!("kubectl --context {kctx} -n {ns} describe pods -l app=ssas-ai"));
        info(&format!("kubectl --context {kctx} -n {ns} logs deploy/ssas-ai --tail=80"));
    }

    if let Some(i) = &ingress {
        let proto = if i.tls.is_some() { "https" } else { "http" };
        ok(&format!("Indirizzo: {proto}://{}", i.host));
        let addr = kcap(&kctx, &["-n", "ingress-nginx", "get", "svc", "-o", "jsonpath={.items[0].status.loadBalancer.ingress[0].ip}"]);
        let addr = addr.trim();
        if !addr.is_empty() && !addr.to_lowercase().contains("error") {
            info(&format!("Punta il record DNS di {} a {addr}", i.host));
        } else {
            info(&format!("Punta il record DNS di {} all'IP esterno dell'ingress controller", i.host));
        }
        if i.tls.is_some() {
            info(&format!("Stato certificato: kubectl --context {kctx} -n {ns} get certificate"));
        }
    } else {
        info(&format!("Accesso locale: kubectl --context {kctx} -n {ns} port-forward svc/ssas-ai 8000:80  →  http://localhost:8000"));
    }
    info(&format!("Log: kubectl --context {kctx} -n {ns} logs -l app=ssas-ai -f"));
    Ok(())
}

// ───────────────────────── manifest ─────────────────────────

fn namespace_yaml(ns: &str) -> String {
    format!("apiVersion: v1\nkind: Namespace\nmetadata:\n  name: {ns}\n  labels:\n    app.kubernetes.io/name: ssas-ai\n")
}

fn secret_yaml(ns: &str, vars: &[(String, String)], pull: Option<&PullSecret>) -> String {
    let mut y = format!(
        "# Contiene segreti in chiaro. NON committare.\napiVersion: v1\nkind: Secret\nmetadata:\n  name: ssas-ai-secret\n  namespace: {ns}\ntype: Opaque\nstringData:\n"
    );
    for (k, v) in vars {
        y.push_str(&format!("  {k}: {}\n", q(v)));
    }
    if let Some(p) = pull {
        let auth = b64(format!("{}:{}", p.user, p.password).as_bytes());
        let cfg = format!(
            "{{\"auths\":{{{}:{{\"username\":{},\"password\":{},\"auth\":\"{}\"}}}}}}",
            q(&p.server), q(&p.user), q(&p.password), auth
        );
        y.push_str(&format!(
            "---\napiVersion: v1\nkind: Secret\nmetadata:\n  name: ssas-ai-registry\n  namespace: {ns}\ntype: kubernetes.io/dockerconfigjson\nstringData:\n  .dockerconfigjson: {}\n",
            q(&cfg)
        ));
    }
    y
}

fn configmap_yaml(ns: &str, vars: &[(String, String)]) -> String {
    let mut y = format!("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: ssas-ai-config\n  namespace: {ns}\ndata:\n");
    for (k, v) in vars {
        y.push_str(&format!("  {k}: {}\n", q(v)));
    }
    y
}

fn sc_line(storage_class: &str) -> String {
    if storage_class.is_empty() {
        String::new()
    } else {
        format!("  storageClassName: {}\n", q(storage_class))
    }
}

fn postgres_yaml(ns: &str, user: &str, name: &str, size: &str, storage_class: &str) -> String {
    let sc = if storage_class.is_empty() { String::new() } else { format!("        storageClassName: {}\n", q(storage_class)) };
    let (user_q, name_q) = (q(user), q(name));
    format!(
        r#"apiVersion: v1
kind: Service
metadata:
  name: ssas-ai-postgres
  namespace: {ns}
spec:
  selector:
    app: ssas-ai-postgres
  ports:
    - name: pg
      port: 5432
      targetPort: 5432
---
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: ssas-ai-postgres
  namespace: {ns}
spec:
  serviceName: ssas-ai-postgres
  replicas: 1
  selector:
    matchLabels:
      app: ssas-ai-postgres
  template:
    metadata:
      labels:
        app: ssas-ai-postgres
    spec:
      containers:
        - name: postgres
          image: postgres:18-alpine
          ports:
            - containerPort: 5432
              name: pg
          env:
            - name: POSTGRES_USER
              value: {user_q}
            - name: POSTGRES_DB
              value: {name_q}
            - name: POSTGRES_PASSWORD
              valueFrom:
                secretKeyRef:
                  name: ssas-ai-secret
                  key: POSTGRES_PASSWORD
          readinessProbe:
            exec:
              command: ["sh", "-c", "pg_isready -U {user} -d {name}"]
            initialDelaySeconds: 5
            periodSeconds: 5
          resources:
            requests:
              cpu: "100m"
              memory: "256Mi"
            limits:
              memory: "1Gi"
          volumeMounts:
            - name: pgdata
              mountPath: /var/lib/postgresql
  volumeClaimTemplates:
    - metadata:
        name: pgdata
      spec:
        accessModes: ["ReadWriteOnce"]
{sc}        resources:
          requests:
            storage: {size}
"#
    )
}

fn pvc_yaml(ns: &str, storage_class: &str, data: &str, logs: &str, rwx: bool) -> String {
    let mode = if rwx { "ReadWriteMany" } else { "ReadWriteOnce" };
    let sc = sc_line(storage_class);
    let one = |name: &str, size: &str| {
        format!(
            "apiVersion: v1\nkind: PersistentVolumeClaim\nmetadata:\n  name: {name}\n  namespace: {ns}\n  labels:\n    app: ssas-ai\nspec:\n  accessModes:\n    - {mode}\n{sc}  resources:\n    requests:\n      storage: {size}\n"
        )
    };
    format!("{}---\n{}", one("ssas-ai-data", data), one("ssas-ai-logs", logs))
}

fn deployment_yaml(ns: &str, image: &str, replicas: u32, rwx: bool, pull_secret: bool, wait_pg: bool) -> String {
    let strategy = if !rwx {
        "    type: Recreate".to_string()
    } else if replicas > 1 {
        "    type: RollingUpdate\n    rollingUpdate:\n      maxSurge: 0\n      maxUnavailable: 1".to_string()
    } else {
        "    type: RollingUpdate\n    rollingUpdate:\n      maxSurge: 1\n      maxUnavailable: 0".to_string()
    };
    let pull_policy = if image.ends_with(":latest") || !image.rsplit('/').next().unwrap_or("").contains(':') {
        "Always"
    } else {
        "IfNotPresent"
    };
    let pull = if pull_secret { "      imagePullSecrets:\n        - name: ssas-ai-registry\n" } else { "" };
    let init = if wait_pg {
        "      initContainers:\n        - name: wait-postgres\n          image: busybox:1.36\n          command: [\"sh\", \"-c\", \"until nc -z ssas-ai-postgres 5432; do echo attendo postgres; sleep 2; done\"]\n          securityContext:\n            allowPrivilegeEscalation: false\n            capabilities:\n              drop: [\"ALL\"]\n"
    } else {
        ""
    };
    format!(
        r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: ssas-ai
  namespace: {ns}
  labels:
    app.kubernetes.io/name: ssas-ai
spec:
  replicas: {replicas}
  selector:
    matchLabels:
      app: ssas-ai
  strategy:
{strategy}
  template:
    metadata:
      labels:
        app: ssas-ai
    spec:
      affinity:
        podAntiAffinity:
          preferredDuringSchedulingIgnoredDuringExecution:
            - weight: 100
              podAffinityTerm:
                labelSelector:
                  matchLabels:
                    app: ssas-ai
                topologyKey: kubernetes.io/hostname
      securityContext:
        runAsNonRoot: true
        runAsUser: 1001
        fsGroup: 1001
        fsGroupChangePolicy: OnRootMismatch
{pull}{init}      containers:
        - name: ssas-ai
          image: {image}
          imagePullPolicy: {pull_policy}
          ports:
            - containerPort: 8000
              name: http
          envFrom:
            - configMapRef:
                name: ssas-ai-config
            - secretRef:
                name: ssas-ai-secret
          env:
            - name: LOG_FILE
              value: /app/logs/ssas-ai.log
            # fidarsi di X-Forwarded-For dell'ingress (serve al rate limit per IP)
            - name: FORWARDED_ALLOW_IPS
              value: "*"
          resources:
            requests:
              cpu: "250m"
              memory: "256Mi"
            limits:
              cpu: "1000m"
              memory: "1Gi"
          livenessProbe:
            httpGet:
              path: /healthz
              port: 8000
            initialDelaySeconds: 20
            periodSeconds: 30
            failureThreshold: 3
          readinessProbe:
            httpGet:
              path: /healthz
              port: 8000
            initialDelaySeconds: 10
            periodSeconds: 10
            failureThreshold: 3
          volumeMounts:
            - name: logs
              mountPath: /app/logs
            - name: data
              mountPath: /app/data
          securityContext:
            allowPrivilegeEscalation: false
            capabilities:
              drop: ["ALL"]
      volumes:
        - name: logs
          persistentVolumeClaim:
            claimName: ssas-ai-logs
        - name: data
          persistentVolumeClaim:
            claimName: ssas-ai-data
"#
    )
}

fn service_yaml(ns: &str, svc_type: &str) -> String {
    format!(
        "apiVersion: v1\nkind: Service\nmetadata:\n  name: ssas-ai\n  namespace: {ns}\nspec:\n  type: {svc_type}\n  selector:\n    app: ssas-ai\n  ports:\n    - name: http\n      port: 80\n      targetPort: 8000\n"
    )
}

fn issuer_yaml(name: &str, email: &str, class: &str) -> String {
    format!(
        r#"apiVersion: cert-manager.io/v1
kind: ClusterIssuer
metadata:
  name: {name}
spec:
  acme:
    server: https://acme-v02.api.letsencrypt.org/directory
    email: {}
    privateKeySecretRef:
      name: {name}-account-key
    solvers:
      - http01:
          ingress:
            ingressClassName: {class}
"#,
        q(email)
    )
}

fn ingress_yaml(ns: &str, i: &Ingress) -> String {
    let cm = i
        .tls
        .as_ref()
        .map(|t| format!("    cert-manager.io/cluster-issuer: {}\n", t.issuer))
        .unwrap_or_default();
    let tls = if i.tls.is_some() {
        format!("  tls:\n    - hosts:\n        - {}\n      secretName: ssas-ai-tls\n", i.host)
    } else {
        String::new()
    };
    // le annotazioni nginx hanno senso solo con il controller nginx
    let nginx = if i.class == "nginx" {
        r#"    nginx.ingress.kubernetes.io/proxy-read-timeout: "600"
    nginx.ingress.kubernetes.io/proxy-send-timeout: "600"
    nginx.ingress.kubernetes.io/proxy-body-size: "10m"
    nginx.ingress.kubernetes.io/proxy-buffering: "off"
    # affinità di sessione: la memoria delle conversazioni è per pod
    nginx.ingress.kubernetes.io/affinity: "cookie"
    nginx.ingress.kubernetes.io/session-cookie-name: "ssasai_route"
    nginx.ingress.kubernetes.io/session-cookie-max-age: "28800"
"#
    } else {
        ""
    };
    format!(
        r#"apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: ssas-ai
  namespace: {ns}
  annotations:
{cm}{nginx}spec:
  ingressClassName: {class}
{tls}  rules:
    - host: {host}
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: ssas-ai
                port:
                  name: http
"#,
        class = i.class,
        host = i.host
    )
}

fn hpa_yaml(ns: &str, min: u32, max: u32) -> String {
    format!(
        r#"apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: ssas-ai
  namespace: {ns}
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: ssas-ai
  minReplicas: {min}
  maxReplicas: {max}
  metrics:
    - type: Resource
      resource:
        name: cpu
        target:
          type: Utilization
          averageUtilization: 70
    - type: Resource
      resource:
        name: memory
        target:
          type: Utilization
          averageUtilization: 80
"#
    )
}

