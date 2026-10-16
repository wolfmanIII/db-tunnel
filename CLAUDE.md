# db-tunnel

CLI Rust che mantiene attivi tunnel locali verso MySQL/PostgreSQL su VM Google Compute Engine, usando `gcloud` come trasporto (`compute ssh` o `compute start-iap-tunnel`). Controlla periodicamente il DB e riavvia il tunnel con backoff se cade.

**Fonte di verità: `docs/db-tunnel-spec.md`.** Leggila prima di implementare o cambiare comportamento. Se codice e spec divergono, chiedi quale vale e aggiorna la spec insieme al codice. Non duplicare qui il contenuto della spec.

## Progetto di studio

Priorità: chiarezza e piccoli passi, non completezza né ottimizzazione.

- Segui il piano per fasi del §12 della spec. Una fase alla volta, ogni fase lascia un binario funzionante. Non anticipare fasi successive.
- Preferisci codice semplice e leggibile ad astrazioni (niente trait/generics/macro se non servono).
- Quando introduci un concetto Rust non banale (lifetime, `select!`, `Pin`, ecc.), spiega in 1-2 righe perché.
- Dipendenze solo quelle del §10 della spec. Altre: chiedi prima.
- Le decisioni aperte (spec §13: `expected_account`, mitigazione `max_connect_errors` in modalità `iap`, default di `mode`) sono di Antonio. Quando il codice dipende da una di esse, fermati e chiedi: non scegliere da solo.

## Comandi

```bash
cargo run -- <args>                         # esegui
cargo test                                  # test
cargo clippy --all-targets -- -D warnings   # deve passare senza warning
cargo fmt                                   # formatta
```

Prima di dichiarare un task finito: `cargo fmt`, `cargo clippy`, `cargo test`.

Test end-to-end contro GCP reale sono manuali: non lanciarli, non servono credenziali nella suite automatica.

### Markdown

Dopo aver modificato `docs/*.md` o `CLAUDE.md`, esegui il lint. Node è gestito con nvm, quindi prima `nvm use --lts`:

```bash
source ~/.nvm/nvm.sh && nvm use --lts && npx -y markdownlint-cli docs/*.md CLAUDE.md HANDOFF.md
```

Config in `.markdownlint.json` (`MD013` line-length disattivata). Blocchi di codice sempre con linguaggio (`bash`, `toml`, `rust`, `text`).

## Architettura (sintesi)

Moduli in `src/`: `main.rs` (CLI, segnali), `config.rs` (TOML, default, validazione), `gcloud.rs` (account attivo, costruzione comandi `gcloud` e `cloud-sql-proxy`), `health.rs` (check MySQL/Postgres), `tunnel.rs` (supervisore a macchina a stati). Dettagli e diagramma degli stati: spec §6.

Invarianti da rispettare:

- Un task tokio per tunnel, indipendenti: l'errore di uno non ferma gli altri.
- Shutdown (Ctrl+C / SIGTERM) via `CancellationToken`; tutti i processi `gcloud` vanno chiusi. Spawn sempre con `kill_on_drop(true)`.
- Nel ciclo `Up`: `tokio::select!` tra timer health check e `child.wait()`.
- La costruzione degli argomenti `gcloud` è una funzione pura (testabile senza eseguire nulla).
- `gcloud auth list` è lento (~1s): chiamarlo solo all'avvio di `up` e in `status`, mai nel loop di check.
- Health check senza credenziali e senza driver, a livello di protocollo (spec §7.4). `Refused(code)` conta come fallimento ma si logga distinto.

## Errori e logging

- `anyhow` + `.context("...")` nel binario. Errori di config, ambiente (gcloud assente, nessun account) e porta occupata: fatali all'avvio. Errori runtime di un tunnel: mai fatali, gestiti dalla macchina a stati.
- Exit code: `0` ok, `1` errori di config/ambiente, `2` se `status` trova tunnel non `UP`.
- Logging con `tracing` (stderr, default `info`, `RUST_LOG`). Niente `println!` per diagnostica. Ogni riga di un tunnel include il suo `name`. Output di `gcloud` riversato nel log con prefisso del nome tunnel.
- Niente `unwrap()`/`expect()` fuori dai test, salvo invarianti documentate da un commento.

## Git

- Identità solo locale al repo (`git config --local`), mai `--global`.
- Nei commit e nelle PR non aggiungere `Co-Authored-By` né altre righe di attribuzione a Claude.
- Repo pubblico: prima di ogni push controlla che non ci siano nomi di VM, progetti o host aziendali reali, né email di lavoro. Negli esempi usa valori fittizi.

## Sicurezza

- Mai loggare credenziali, token o contenuto dei pacchetti DB.
- Bind/check solo su `127.0.0.1`.
- Non committare config reali (`~/.config/db-tunnel/config.toml` sta fuori dal repo; esempi con valori fittizi).

## Test

- Unit test nello stesso file (`#[cfg(test)]`). Copertura attesa per modulo: spec §11.
- Test di rete con `TcpListener` su `127.0.0.1:0` (porta effimera), `#[tokio::test]`.
- Il backoff (1, 2, 4, 8, 16, 32, 60s; reset dopo 5 min in `Up`) è una funzione pura testata a parte.

## Stato

<!-- Aggiorna a ogni fase completata -->
Fasi 1 e 2 completate. Fase 3 (health check) in corso. Dettagli, decisioni prese e cose rimandate in `HANDOFF.md`: leggilo a inizio sessione e aggiornalo a fine lavoro. Piano completo: spec §12.
