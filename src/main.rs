mod gcloud;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match gcloud::active_account().await? {
        Some(acc) => println!("gcloud: loggato come {acc}"),
        None => anyhow::bail!("nessun account gcloud attivo, lancia `gcloud auth login`"),
    }
    Ok(())
}
