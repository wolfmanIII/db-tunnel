use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const POSTGRES_SSL_REQUEST: [u8; 8] = [0x00, 0x00, 0x00, 0x08, 0x04, 0xD2, 0x16, 0x2F];
const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug)]
pub enum Health {
    Ok,
    Refused(u16),
    Down(String),
}

async fn try_postgres(port: u16) -> std::io::Result<Health> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
    stream.write_all(&POSTGRES_SSL_REQUEST).await?;
    let mut risposta = [0u8; 1];
    stream.read_exact(&mut risposta).await?;
    match risposta[0] {
        b'S' | b'N' => Ok(Health::Ok),
        altro => Ok(Health::Down(format!("risposta inattesa: {altro}"))),
    }
}

pub async fn check_postgres(port: u16) -> Health {
    match timeout(TIMEOUT, try_postgres(port)).await {
        Ok(Ok(health)) => health,
        Ok(Err(e)) => Health::Down(e.to_string()),
        Err(_) => Health::Down("timeout".to_string()),
    }
}

async fn try_mysql(port: u16) -> std::io::Result<Health> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
    let mut intestazione = [0u8; 4];
    stream.read_exact(&mut intestazione).await?;
    let mut contenuto = [0u8; 3];
    stream.read_exact(&mut contenuto).await?;
    match contenuto[0] {
        0x0A => Ok(Health::Ok),
        0xFF => Ok(Health::Refused(u16::from_le_bytes([
            contenuto[1],
            contenuto[2],
        ]))),
        altro => Ok(Health::Down(format!("risposta inattesa: {altro}"))),
    }
}

pub async fn check_mysql(port: u16) -> Health {
    match timeout(TIMEOUT, try_mysql(port)).await {
        Ok(Ok(health)) => health,
        Ok(Err(e)) => Health::Down(e.to_string()),
        Err(_) => Health::Down("timeout".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// Finto server: accetta una connessione, scrive `risposta` e poi aspetta che il client chiuda.
    /// Ritorna la porta su cui è in ascolto (porta effimera scelta dal sistema con `:0`).
    async fn finto_server(risposta: Vec<u8>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let porta = listener.local_addr().unwrap().port();
        // `tokio::spawn` vuole un task che non prenda in prestito nulla dall'esterno ('static):
        // con `async move` il task diventa proprietario di `listener` e `risposta`.
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = stream.write_all(&risposta).await;
            let mut buf = [0u8; 16];
            let _ = stream.read(&mut buf).await;
        });
        porta
    }

    /// Finto server che accetta la connessione e la chiude subito.
    async fn server_che_chiude() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let porta = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            // `let _ =` rilascia subito il valore: la connessione viene chiusa senza scrivere nulla.
            let _ = listener.accept().await;
        });
        porta
    }

    /// Una porta su cui non ascolta nessuno: si apre un listener e lo si lascia cadere.
    async fn porta_chiusa() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    }

    /// Pacchetto MySQL: 3 byte di lunghezza (little-endian), 1 di sequenza, poi il contenuto.
    fn pacchetto_mysql(contenuto: &[u8]) -> Vec<u8> {
        let mut pacchetto = vec![contenuto.len() as u8, 0, 0, 0];
        pacchetto.extend_from_slice(contenuto);
        pacchetto
    }

    // `Health` non ha `PartialEq`, quindi si controlla la variante con `matches!`.

    #[tokio::test]
    async fn mysql_handshake_valido() {
        // protocol version 10, poi una versione finta del server
        let porta = finto_server(pacchetto_mysql(&[0x0A, b'8', b'.', b'0', 0])).await;
        let stato = check_mysql(porta).await;
        assert!(matches!(stato, Health::Ok), "{stato:?}");
    }

    #[tokio::test]
    async fn mysql_errore_1129() {
        // 0xFF = ERR packet, poi il codice 1129 (0x0469) in little-endian
        let porta = finto_server(pacchetto_mysql(&[0xFF, 0x69, 0x04])).await;
        let stato = check_mysql(porta).await;
        assert!(matches!(stato, Health::Refused(1129)), "{stato:?}");
    }

    #[tokio::test]
    async fn mysql_risposta_inattesa() {
        let porta = finto_server(pacchetto_mysql(&[0x00, 0x00, 0x00])).await;
        let stato = check_mysql(porta).await;
        assert!(matches!(stato, Health::Down(_)), "{stato:?}");
    }

    #[tokio::test]
    async fn mysql_chiude_subito() {
        let porta = server_che_chiude().await;
        let stato = check_mysql(porta).await;
        assert!(matches!(stato, Health::Down(_)), "{stato:?}");
    }

    #[tokio::test]
    async fn mysql_porta_chiusa() {
        let porta = porta_chiusa().await;
        let stato = check_mysql(porta).await;
        assert!(matches!(stato, Health::Down(_)), "{stato:?}");
    }

    /// Il server accetta ma non scrive mai: dopo 3 secondi di attesa il check si arrende.
    #[tokio::test]
    async fn mysql_silenzio_va_in_timeout() {
        let porta = finto_server(Vec::new()).await;
        let stato = check_mysql(porta).await;
        assert!(
            matches!(&stato, Health::Down(m) if m == "timeout"),
            "{stato:?}"
        );
    }

    #[tokio::test]
    async fn postgres_risponde_n() {
        let porta = finto_server(vec![b'N']).await;
        let stato = check_postgres(porta).await;
        assert!(matches!(stato, Health::Ok), "{stato:?}");
    }

    #[tokio::test]
    async fn postgres_risponde_s() {
        let porta = finto_server(vec![b'S']).await;
        let stato = check_postgres(porta).await;
        assert!(matches!(stato, Health::Ok), "{stato:?}");
    }

    #[tokio::test]
    async fn postgres_risposta_inattesa() {
        let porta = finto_server(vec![b'X']).await;
        let stato = check_postgres(porta).await;
        assert!(matches!(stato, Health::Down(_)), "{stato:?}");
    }

    #[tokio::test]
    async fn postgres_chiude_subito() {
        let porta = server_che_chiude().await;
        let stato = check_postgres(porta).await;
        assert!(matches!(stato, Health::Down(_)), "{stato:?}");
    }

    #[tokio::test]
    async fn postgres_porta_chiusa() {
        let porta = porta_chiusa().await;
        let stato = check_postgres(porta).await;
        assert!(matches!(stato, Health::Down(_)), "{stato:?}");
    }
}
