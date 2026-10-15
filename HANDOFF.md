# HANDOFF

Stato del lavoro, per riprendere in una nuova sessione. Aggiornalo a fine di ogni fase.

Riferimenti: `docs/db-tunnel-spec.md` (specifica), `CLAUDE.md` (regole di lavoro).

## Dove siamo

- Fase 1 (account gcloud): completata.
- Fase 2 (config): completata. `src/config.rs` legge e valida il TOML, 17 test; `main` stampa i tunnel letti e l'account gcloud.
- Prossima: fase 3 (health check e comando `status`), vedi spec §7.4 e §11.

## Decisioni prese

- Modalità `cloudsql` aggiunta alla spec: usa `cloud-sql-proxy` con `--gcloud-auth`, quindi lo stesso account di gcloud (nessun secondo login).
- `mode` è obbligatorio: se manca, errore che indica il tunnel.
- Il file di config deve avere almeno un tunnel e non ammette campi sconosciuti.
- Percorso del config: argomento opzionale in `main` finché non c'è clap (fase 6); default `~/.config/db-tunnel/config.toml`.

## Da fare (rimandato)

- **`expected_account`** (spec §13): campo opzionale per tunnel, rifiuta l'avvio se l'account gcloud attivo è diverso. Rimandato dopo la fase 2. Va aggiunto a `RawTunnel` e `Tunnel`, con validazione e test. Essendo opzionale non rompe i file di config esistenti.
- **Mitigazione `max_connect_errors`** per MySQL in modalità `iap` o `cloudsql` (spec §7.5, §13). Poco urgente: il caso d'uso attuale è Postgres.
- Da valutare: validare i `settings` (valori a 0 farebbero un ciclo di check senza pause). Non è nella spec.
- Test manuale del proxy contro il database vero: fase 4. I dati reali (progetto, istanza, IP) vanno solo in `~/.config/db-tunnel/config.toml`, mai nel repo (pubblico).

## Ambiente

- `cloud-sql-proxy` v2.26.2 installato in `~/.local/bin`.
- Node via nvm: `source ~/.nvm/nvm.sh && nvm use --lts`, poi `npx -y markdownlint-cli docs/*.md CLAUDE.md HANDOFF.md`.
- Repo GitHub pubblico, branch `main`. Identità git solo locale al repo. Niente `Co-Authored-By`.
