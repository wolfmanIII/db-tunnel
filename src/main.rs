use std::path::PathBuf;

mod config;
mod gcloud;
mod health;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Per ora il percorso è un argomento opzionale; `--config` con clap arriva alla fase 6.
    let path = match std::env::args().nth(1) {
        Some(p) => PathBuf::from(p),
        None => config::default_path()?,
    };
    let config = config::load(&path)?;

    println!(
        "config: {} tunnel letti da {}",
        config.tunnels.len(),
        path.display()
    );
    println!(
        "settings: check ogni {}s, soglia {} fallimenti, avvio entro {}s",
        config.settings.check_interval_secs,
        config.settings.failure_threshold,
        config.settings.startup_timeout_secs
    );
    for t in &config.tunnels {
        println!(
            "  {} [{:?}] localhost:{} <- {}",
            t.name, t.engine, t.local_port, t.target
        );
        let stato = health::check_postgres(t.local_port).await;
        println!("    stato: {stato:?}");
    }

    match gcloud::active_account().await? {
        Some(acc) => println!("gcloud: loggato come {acc}"),
        None => anyhow::bail!("nessun account gcloud attivo, lancia `gcloud auth login`"),
    }
    Ok(())
}
