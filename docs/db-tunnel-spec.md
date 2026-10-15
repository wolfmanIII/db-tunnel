# db-tunnel — Specifica tecnica

## 1. Obiettivo

CLI in Rust che apre e mantiene attivi tunnel locali verso database MySQL e PostgreSQL su Google Cloud, in due casi: database installati su VM Compute Engine (trasporto `gcloud`) e istanze Cloud SQL (trasporto Cloud SQL Auth Proxy). Il tool verifica periodicamente che il database risponda e, se il tunnel cade o il DB non è raggiungibile, lo riavvia automaticamente.

È anche un progetto di studio: le scelte privilegiano chiarezza e piccoli passi rispetto a completezza e ottimizzazione.

## 2. Perimetro

### Incluso

- Tunnel verso VM Compute Engine tramite `gcloud compute ssh` (port forwarding SSH) o `gcloud compute start-iap-tunnel` (IAP TCP forwarding).
- Tunnel verso istanze Cloud SQL tramite Cloud SQL Auth Proxy (`cloud-sql-proxy`), con connessione all'IP pubblico dell'istanza.
- Più tunnel definiti in un unico file di configurazione, gestiti in parallelo.
- Health check per MySQL e PostgreSQL a livello di protocollo, senza credenziali.
- Riavvio automatico con backoff.
- Visualizzazione dell'account gcloud attivo.
- Comandi `up` e `status`.

### Escluso (per ora)

- Cloud SQL con solo IP privato (richiede `--private-ip` e una rete raggiungibile), altri cloud provider, altri database.
- Health check autenticato (`SELECT 1`) — possibile estensione futura.
- Demonizzazione propria: per l'esecuzione in background si userà systemd.
- Interfaccia grafica o TUI.

## 3. Prerequisiti di runtime

- `gcloud` installato e presente nel `PATH`.
- Almeno un account autenticato (`gcloud auth login`).
- Permessi IAM sufficienti (a seconda della modalità):
  - `ssh` / `iap`: accesso SSH o IAP alla VM.
  - `cloudsql`: ruolo "Cloud SQL Client" sul progetto e Cloud SQL Admin API abilitata.
- Solo per `mode = "cloudsql"`:
  - `cloud-sql-proxy` (v2) presente nel `PATH`, scaricato dalle release ufficiali del Cloud SQL Auth Proxy.
  - Nessun secondo login: il proxy usa l'account attivo di gcloud grazie al flag `--gcloud-auth` (presente nella v2.26.2). L'account mostrato da `status` (§7.1) è quindi lo stesso usato dal proxy.

## 4. Configurazione

### 4.1 Posizione

- Default: `~/.config/db-tunnel/config.toml`
- Override: opzione `--config <path>`

### 4.2 Formato

```toml
[settings]
check_interval_secs = 10      # intervallo tra health check
failure_threshold = 3         # check falliti consecutivi prima del riavvio
startup_timeout_secs = 20     # attesa massima perché il tunnel diventi raggiungibile

[[tunnel]]
name = "prod-pg"
engine = "postgres"           # "mysql" | "postgres"
mode = "ssh"                  # "ssh" | "iap" | "cloudsql"
project = "my-project"
zone = "europe-west8-a"
instance = "db-vm-1"
local_port = 15432
remote_host = "127.0.0.1"     # opzionale, default 127.0.0.1 (solo mode ssh)
remote_port = 5432            # opzionale, default per engine: 3306 / 5432

[[tunnel]]
name = "test-pg"
engine = "postgres"
mode = "iap"
project = "my-project"
zone = "europe-west8-b"
instance = "my-test-vm"
local_port = 15433

[[tunnel]]
name = "app-pg"
engine = "postgres"
mode = "cloudsql"
instance_connection_name = "my-project:europe-west8:my-pg-db"   # progetto:regione:istanza
local_port = 15434
```

`instance_connection_name` si legge nella console Cloud SQL o con `gcloud sql instances describe <istanza> --format="value(connectionName)"`. Usa la **regione**, non la zona.

### 4.3 Regole di validazione

- `name` univoco tra i tunnel.
- `local_port` univoca tra i tunnel e compresa tra 1024 e 65535.
- `remote_host` ammesso solo con `mode = "ssh"`.
- `mode` è obbligatorio: se manca, errore che indica il tunnel.
- Il file deve definire almeno un tunnel.
- Campi sconosciuti (ad esempio per un refuso) sono un errore.
- Campi per modalità:

  | Modalità | Obbligatori | Non ammessi |
  | --- | --- | --- |
  | `ssh` | `project`, `zone`, `instance` | `instance_connection_name` |
  | `iap` | `project`, `zone`, `instance` | `instance_connection_name`, `remote_host` |
  | `cloudsql` | `instance_connection_name` | `project`, `zone`, `instance`, `remote_host`, `remote_port` |

- `instance_connection_name` deve avere la forma `progetto:regione:istanza` (tre parti non vuote separate da `:`).
- Errori di validazione bloccano l'avvio con un messaggio che indica il tunnel e il campo coinvolti.

## 5. Interfaccia a riga di comando

```text
db-tunnel up [--config <path>] [--only <name>...]
db-tunnel status [--config <path>]
```

**`up`** — avvia tutti i tunnel (o solo quelli indicati), resta in foreground, logga gli eventi, termina pulitamente su Ctrl+C / SIGTERM chiudendo tutti i processi figli.

**`status`** — comando stateless: non comunica con un eventuale `up` in esecuzione. Mostra l'account gcloud attivo ed esegue un health check su ogni `local_port` della config, riportando per ciascun tunnel `UP`, `DOWN` o `RIFIUTA (errore N)`.

Exit code: `0` se tutto ok, `1` per errori di configurazione o di ambiente, `2` se `status` trova almeno un tunnel non `UP` (utile negli script).

## 6. Architettura

### 6.1 Moduli

```text
src/
  main.rs      # parsing CLI, avvio, gestione segnali
  config.rs    # struct di configurazione, defaults, validazione
  gcloud.rs    # account attivo, costruzione comandi (gcloud e cloud-sql-proxy)
  health.rs    # health check MySQL / PostgreSQL
  tunnel.rs    # supervisore del singolo tunnel (macchina a stati)
```

### 6.2 Modello di esecuzione

- Runtime `tokio`.
- `up` crea un task asincrono per ogni tunnel; i task sono indipendenti, il fallimento di uno non influenza gli altri.
- Il task principale attende Ctrl+C / SIGTERM e poi chiede a tutti i task di terminare (`tokio_util::sync::CancellationToken` o canale `broadcast`).
- I processi figli (`gcloud`, `cloud-sql-proxy`) sono creati con `kill_on_drop(true)` come rete di sicurezza.

### 6.3 Macchina a stati del tunnel

```text
Starting ──(porta raggiungibile + check ok)──▶ Up
Starting ──(timeout o processo terminato)───▶ Backoff
Up ───────(failure_threshold check falliti)─▶ Restarting
Up ───────(processo del tunnel terminato)───▶ Backoff
Restarting ─(kill del processo)─────────────▶ Backoff
Backoff ──(attesa)──────────────────────────▶ Starting
```

Nel ciclo `Up` il task usa `tokio::select!` tra il timer dell'health check e `child.wait()`, così rileva subito la morte del processo senza attendere il check successivo.

### 6.4 Backoff

- Esponenziale: 1, 2, 4, 8, 16, 32, 60 secondi (tetto a 60).
- Il contatore si azzera dopo 5 minuti consecutivi in stato `Up`.

## 7. Componenti

### 7.1 Account gcloud (`gcloud.rs`)

```bash
gcloud auth list --filter=status:ACTIVE --format=value(account)
```

- Ritorna `Option<String>`: `None` se nessun account è attivo.
- `None` all'avvio di `up` è un errore bloccante con suggerimento `gcloud auth login`.
- Chiamato solo all'avvio di `up` e in `status`, mai nel loop di health check (avvio di gcloud lento, circa 1s).
- Se `gcloud` non è nel `PATH`, errore esplicito e chiaro.

### 7.2 Comandi del tunnel (`gcloud.rs`)

Funzione pura che, data la configurazione di un tunnel, restituisce programma e argomenti (testabile senza eseguire nulla).

Modalità `ssh`:

```bash
gcloud compute ssh <instance> --project <project> --zone <zone> \
  -- -N -L <local_port>:<remote_host>:<remote_port> \
  -o ExitOnForwardFailure=yes -o ServerAliveInterval=15 -o ServerAliveCountMax=3
```

Modalità `iap`:

```bash
gcloud compute start-iap-tunnel <instance> <remote_port> \
  --local-host-port=localhost:<local_port> --project <project> --zone <zone>
```

Modalità `cloudsql` (il programma è `cloud-sql-proxy`, non `gcloud`):

```bash
cloud-sql-proxy <instance_connection_name> --gcloud-auth --address 127.0.0.1 --port <local_port>
```

Il proxy ascolta in locale e apre la connessione verso Cloud SQL solo quando arriva un client, quindi la porta locale può essere raggiungibile anche se l'istanza non risponde: per questo il controllo di prontezza è l'health check (§7.4) e non la sola porta aperta.

`stdout` e `stderr` del processo figlio vanno catturati e riversati nel log con il nome del tunnel come prefisso, così gli errori (permessi, VM spenta, istanza Cloud SQL ferma, ecc.) sono visibili.

### 7.3 Controllo porta locale

Prima di avviare un tunnel, verificare che `local_port` sia libera (`TcpListener::bind` su `127.0.0.1:<port>` e rilascio immediato). Se è occupata: errore con indicazione della porta e del tunnel, nessun tentativo di avvio.

### 7.4 Health check (`health.rs`)

Connessione TCP a `127.0.0.1:<local_port>`, timeout di 3s su connect e su ogni read. Nessuna credenziale, nessun driver.

Con `cloudsql` il check attraversa il proxy fino al database vero: ogni check apre una connessione reale verso Cloud SQL, quindi verifica tutto il percorso (credenziali del proxy, rete, istanza attiva).

**MySQL** — il server invia per primo il pacchetto di handshake: 3 byte di lunghezza (little-endian), 1 byte di sequenza, poi il payload.

- Payload che inizia con `0x0A` (protocol version 10): DB attivo → `Ok`.
- Payload che inizia con `0xFF` (ERR packet): leggere i 2 byte successivi come codice di errore little-endian → `Refused(code)`. Il server è vivo ma rifiuta la connessione (es. 1129, host bloccato).
- Qualsiasi altra cosa, timeout o connessione chiusa → `Down`.

**PostgreSQL** — il client parla per primo. Inviare una `SSLRequest`:

```text
[0x00, 0x00, 0x00, 0x08, 0x04, 0xD2, 0x16, 0x2F]   // length=8, code=80877103
```

- Risposta di un byte `S` o `N` → `Ok`.
- Qualsiasi altra cosa, timeout o connessione chiusa → `Down`.

Tipo di ritorno suggerito:

```rust
enum Health { Ok, Refused(u16), Down(String) }
```

`Refused` conta come fallimento ai fini del `failure_threshold`, ma va loggato distintamente perché riavviare il tunnel non lo risolve.

### 7.5 Rischi noti dell'health check

- **MySQL `max_connect_errors`**: ogni check chiude la connessione a metà handshake, e MySQL la conta come errore di connessione. Superata la soglia (default 100) l'host viene bloccato (errore 1129). Le connessioni da loopback (`127.0.0.1`/`::1`) sono escluse da questo meccanismo, quindi in modalità `ssh` con `remote_host = "127.0.0.1"` il problema non si pone. In modalità `iap` o `cloudsql`, o con `remote_host` non loopback, le connessioni arrivano da un IP remoto e il blocco è realistico (con check ogni 10s si arriva a 100 in meno di 20 minuti). Mitigazioni possibili, da decidere: alzare `max_connect_errors` sul server, intervallo di check più lungo, o health check autenticato.
- **PostgreSQL**: chiudere la connessione dopo la `SSLRequest` può generare nel log del server righe tipo "incomplete startup packet". Non è dannoso, solo rumore.
- **Cloud SQL, istanze piccole**: ogni check consuma una connessione reale per pochi istanti. Su tier piccoli (es. `db-f1-micro`, limite basso di connessioni) non abbassare troppo `check_interval_secs` e non aprire più tunnel verso la stessa istanza senza necessità.

## 8. Logging

- Crate `tracing` + `tracing-subscriber`, output su stderr con timestamp.
- Livello di default `info`, configurabile con la variabile d'ambiente `RUST_LOG`.
- Ogni riga relativa a un tunnel include il suo `name`.
- Eventi da loggare almeno: avvio, `Up`, check fallito (`debug` il singolo, `warn` al raggiungimento della soglia), `Refused`, riavvio, attesa di backoff, uscita del processo del tunnel (`gcloud` o `cloud-sql-proxy`) con exit code, shutdown.

## 9. Gestione errori

- `anyhow` nel binario (main, orchestrazione) per propagare errori con contesto (`.context("...")`).
- Errori di configurazione, di ambiente (gcloud assente, nessun account, `cloud-sql-proxy` assente se c'è un tunnel `cloudsql`) e di porta occupata sono fatali all'avvio.
- Errori a runtime di un singolo tunnel non sono mai fatali: si gestiscono con la macchina a stati.

## 10. Dipendenze

| Crate | Uso |
| --- | --- |
| `tokio` (features `full`) | runtime async, processi, rete, timer, segnali |
| `anyhow` | gestione errori |
| `clap` (feature `derive`) | parsing CLI |
| `serde` (feature `derive`) | deserializzazione config |
| `toml` | parsing del file di configurazione |
| `tracing`, `tracing-subscriber` | logging |
| `dirs` | percorso `~/.config` |
| `tokio-util` | `CancellationToken` per lo shutdown |

## 11. Test

- **config**: parsing di esempi validi, applicazione dei default (`remote_port` per engine), errori di validazione (nomi o porte duplicate, `remote_host` con `iap`, campi obbligatori o vietati per ogni modalità, `instance_connection_name` malformato).
- **gcloud**: costruzione degli argomenti per le tre modalità, verificata come lista di stringhe.
- **health**: test `#[tokio::test]` con un `TcpListener` locale che simula il server:
  - MySQL che invia handshake valido → `Ok`
  - MySQL che invia ERR 1129 → `Refused(1129)`
  - Postgres che risponde `N` → `Ok`
  - server che chiude subito o non risponde → `Down`
- **tunnel**: logica di backoff testata come funzione pura (sequenza dei ritardi e reset).

Test end-to-end contro GCP reale: manuali, fuori dalla suite automatica.

## 12. Piano di implementazione

Ogni fase produce un binario funzionante.

1. **Account gcloud** — `main` stampa l'account attivo o un errore chiaro. ✅ completata
2. **Config** — lettura e validazione del TOML (tre modalità, con i campi per modalità del §4.3); `main` stampa i tunnel letti. Test di config. ✅ completata
3. **Health check** — `health.rs` con i test del §11; comando `status` completo (account + check delle porte).
4. **Avvio singolo tunnel** — spawn del processo del tunnel (`gcloud` o `cloud-sql-proxy`), controllo porta libera, attesa che la porta diventi raggiungibile, log dell'output di gcloud. Ctrl+C chiude il processo.
5. **Supervisione** — macchina a stati, health check periodico, riavvio con backoff.
6. **Più tunnel e CLI** — un task per tunnel, sottocomandi `up`/`status` con `clap`, opzione `--only`, shutdown coordinato, logging con `tracing`.
7. **(Opzionale) systemd** — unit file di esempio in `docs/`.

## 13. Decisioni aperte

- **`expected_account`** per tunnel: rifiutare l'avvio se l'account gcloud attivo non corrisponde.
- **Mitigazione `max_connect_errors`** per MySQL in modalità `iap` o `cloudsql` (vedi §7.5).
