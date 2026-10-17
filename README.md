# ssas-installer

Installazione guidata di **SSAS AI** con interfaccia grafica (Windows x64). Un solo eseguibile, nessun runtime: chiede tutti i parametri, scrive i file necessari ed esegue l'installazione, mostrando l'avanzamento in tempo reale.

*Guided installer with a graphical UI for SSAS AI (Windows x64). One executable, no runtime needed.*

| Modalità | Cosa fa |
|---|---|
| **Docker** | Verifica Docker/Compose, build (o pull) dell'immagine, scrive `.env` + `docker-compose.yml` (app + PostgreSQL 18 incluso o esterno), avvia, attende l'healthcheck e mostra la password iniziale di `admin` |
| **Kubernetes** | Sceglie il context, scrive i manifest (namespace, secret, configmap, PostgreSQL, PVC, deployment, service, ingress, HPA, ClusterIssuer), può installare ingress-nginx e cert-manager con Helm, `kubectl apply` e attesa del rollout |
| **Windows** | App nativa come servizio: lancia gli script PowerShell di SSAS AI (`setup-prerequisites.ps1`, `install.ps1`) con i parametri raccolti e un `.env` già preconfigurato |
| **Proxy SSAS** | Sul server Windows con SSAS: copia `ssas_proxy.py`, scrive `config.json`, installa il servizio NSSM o l'attività pianificata e verifica `/health` |
| **Aggiorna** | Aggiorna un'installazione esistente (Docker, Kubernetes con rollback, Windows, proxy) lasciando invariati `.env`, volumi e token |

## Uso

Scarica `ssas-installer-<versione>-windows-x64.exe` dalle [Release](../../releases) e avvialo. Nella barra laterale indica la cartella del progetto SSAS AI (quella con `backend` e `frontend`), necessaria per le build Docker e per gli script Windows. Windows e Proxy richiedono un'istanza avviata come Amministratore. L'interruttore **Solo simulazione** scrive i file senza eseguire comandi.

Verifica il download con `SHA256SUMS.txt` (`Get-FileHash <file> -Algorithm SHA256`).

## Compilare

```powershell
cargo build --release          # Rust stable; su Windows servono i Build Tools di Visual Studio
.\target\release\ssas-installer.exe
cargo test                     # esegue tutte le modalità in simulazione
```

Con `SSAS_TEST_OUT=<cartella>` i test scrivono i file generati, verificabili con `docker compose config` e `kubectl apply --dry-run=client --validate=false -f`.

## Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

- **Cosa viene firmato / What is signed**: only the `ssas-installer.exe` release binary, built from the sources of this repository by the GitHub Actions workflow [`.github/workflows/release.yml`](.github/workflows/release.yml) on a GitHub-hosted runner. No third-party binaries are signed.
- **Ruoli / Roles**: committers and reviewers: [Lucio Gambelunghe](https://github.com/RedHombre). Approvers of signing requests: [Lucio Gambelunghe](https://github.com/RedHombre).
- **Privacy**: this program does not collect or transmit any data about its users. It runs the commands you choose (Docker, kubectl, Helm, PowerShell scripts) on your own machine and only writes the files you configure.

Fino all'approvazione di SignPath le Release possono contenere un eseguibile **non firmato**: con Smart App Control attivo Windows 11 può bloccarlo.

## Licenza

[MIT](LICENSE)
