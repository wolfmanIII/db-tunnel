use std::io::ErrorKind;

use anyhow::{Context, Result};
use tokio::process::Command;

/// Account gcloud attivo, `None` se nessun account è loggato.
pub async fn active_account() -> Result<Option<String>> {
    let out = Command::new("gcloud")
        .args([
            "auth",
            "list",
            "--filter=status:ACTIVE",
            "--format=value(account)",
        ])
        .output()
        .await
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => {
                anyhow::anyhow!("`gcloud` non trovato nel PATH: installa la Google Cloud CLI")
            }
            _ => anyhow::Error::new(e).context("impossibile eseguire `gcloud`"),
        })?;
    anyhow::ensure!(
        out.status.success(),
        "gcloud ha fallito: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    let stdout = String::from_utf8(out.stdout).context("output di gcloud non UTF-8")?;
    Ok(parse_account(&stdout))
}

fn parse_account(stdout: &str) -> Option<String> {
    let acc = stdout.trim();
    (!acc.is_empty()).then(|| acc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_presente() {
        assert_eq!(
            parse_account("mario@example.com\n"),
            Some("mario@example.com".to_string())
        );
    }

    #[test]
    fn nessun_account() {
        assert_eq!(parse_account(""), None);
        assert_eq!(parse_account("\n"), None);
    }
}
