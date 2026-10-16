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
