use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

const DEFAULT_REMOTE_HOST: &str = "127.0.0.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Mysql,
    Postgres,
}

impl Engine {
    fn default_port(self) -> u16 {
        match self {
            Engine::Mysql => 3306,
            Engine::Postgres => 5432,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Ssh,
    Iap,
    Cloudsql,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Ssh => "ssh",
            Mode::Iap => "iap",
            Mode::Cloudsql => "cloudsql",
        }
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub check_interval_secs: u64,
    pub failure_threshold: u32,
    pub startup_timeout_secs: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            check_interval_secs: 10,
            failure_threshold: 3,
            startup_timeout_secs: 20,
        }
    }
}

/// Configurazione già validata: i campi sono quelli giusti per la modalità.
#[derive(Debug, PartialEq, Eq)]
pub struct Config {
    pub settings: Settings,
    pub tunnels: Vec<Tunnel>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Tunnel {
    pub name: String,
    pub engine: Engine,
    pub local_port: u16,
    pub target: Target,
}

/// Ogni modalità porta con sé solo i campi che le servono, così nel resto
/// del programma non esistono combinazioni impossibili (es. `zone` con cloudsql).
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    Ssh {
        project: String,
        zone: String,
        instance: String,
        remote_host: String,
        remote_port: u16,
    },
    Iap {
        project: String,
        zone: String,
        instance: String,
        remote_port: u16,
    },
    CloudSql {
        instance_connection_name: String,
    },
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Ssh {
                project,
                zone,
                instance,
                remote_host,
                remote_port,
            } => write!(
                f,
                "ssh {project}/{zone}/{instance} -> {remote_host}:{remote_port}"
            ),
            Target::Iap {
                project,
                zone,
                instance,
                remote_port,
            } => write!(f, "iap {project}/{zone}/{instance} porta {remote_port}"),
            Target::CloudSql {
                instance_connection_name,
            } => write!(f, "cloudsql {instance_connection_name}"),
        }
    }
}

/// Forma del TOML così com'è scritta nel file: quasi tutto è `Option`,
/// perché quali campi servono dipende da `mode`. Si controlla in `validate`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    settings: Settings,
    #[serde(default, rename = "tunnel")]
    tunnels: Vec<RawTunnel>,
}

// Le porte sono `i64` (il tipo dei numeri TOML) e non `u16`: così un valore
// fuori intervallo arriva a `validate`, che sa dire in quale tunnel è sbagliato.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTunnel {
    name: String,
    engine: Engine,
    mode: Option<Mode>,
    local_port: i64,
    project: Option<String>,
    zone: Option<String>,
    instance: Option<String>,
    instance_connection_name: Option<String>,
    remote_host: Option<String>,
    remote_port: Option<i64>,
}

impl RawTunnel {
    fn build(self) -> Result<Tunnel, String> {
        let mode = self
            .mode
            .ok_or("manca il campo `mode` (ssh, iap o cloudsql)")?;
        let m = mode.as_str();
        let local_port = port(self.local_port, 1024, "local_port")?;

        let target = match mode {
            Mode::Ssh => {
                forbidden(
                    &self.instance_connection_name,
                    "instance_connection_name",
                    m,
                )?;
                Target::Ssh {
                    project: required(self.project, "project", m)?,
                    zone: required(self.zone, "zone", m)?,
                    instance: required(self.instance, "instance", m)?,
                    remote_host: self
                        .remote_host
                        .unwrap_or_else(|| DEFAULT_REMOTE_HOST.to_string()),
                    remote_port: remote_port(self.remote_port, self.engine)?,
                }
            }
            Mode::Iap => {
                forbidden(
                    &self.instance_connection_name,
                    "instance_connection_name",
                    m,
                )?;
                forbidden(&self.remote_host, "remote_host", m)?;
                Target::Iap {
                    project: required(self.project, "project", m)?,
                    zone: required(self.zone, "zone", m)?,
                    instance: required(self.instance, "instance", m)?,
                    remote_port: remote_port(self.remote_port, self.engine)?,
                }
            }
            Mode::Cloudsql => {
                forbidden(&self.project, "project", m)?;
                forbidden(&self.zone, "zone", m)?;
                forbidden(&self.instance, "instance", m)?;
                forbidden(&self.remote_host, "remote_host", m)?;
                forbidden(&self.remote_port, "remote_port", m)?;
                let name = required(self.instance_connection_name, "instance_connection_name", m)?;
                check_connection_name(&name)?;
                Target::CloudSql {
                    instance_connection_name: name,
                }
            }
        };

        Ok(Tunnel {
            name: self.name,
            engine: self.engine,
            local_port,
            target,
        })
    }
}

fn port(value: i64, min: u16, field: &str) -> Result<u16, String> {
    u16::try_from(value)
        .ok()
        .filter(|&p| p >= min)
        .ok_or_else(|| format!("`{field}` = {value} fuori dall'intervallo {min}-65535"))
}

fn remote_port(value: Option<i64>, engine: Engine) -> Result<u16, String> {
    match value {
        Some(v) => port(v, 1, "remote_port"),
        None => Ok(engine.default_port()),
    }
}

fn required(value: Option<String>, field: &str, mode: &str) -> Result<String, String> {
    value
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("il campo `{field}` è obbligatorio con mode = \"{mode}\""))
}

fn forbidden<T>(value: &Option<T>, field: &str, mode: &str) -> Result<(), String> {
    if value.is_some() {
        Err(format!(
            "il campo `{field}` non è ammesso con mode = \"{mode}\""
        ))
    } else {
        Ok(())
    }
}

fn check_connection_name(value: &str) -> Result<(), String> {
    let parts: Vec<&str> = value.split(':').collect();
    if parts.len() == 3 && parts.iter().all(|p| !p.trim().is_empty()) {
        Ok(())
    } else {
        Err(format!(
            "`instance_connection_name` = \"{value}\" non valido: atteso progetto:regione:istanza"
        ))
    }
}

fn validate(raw: RawConfig) -> Result<Config> {
    ensure!(
        !raw.tunnels.is_empty(),
        "nessun tunnel definito: aggiungi almeno una sezione [[tunnel]]"
    );

    let mut tunnels = Vec::new();
    for raw_tunnel in raw.tunnels {
        let name = raw_tunnel.name.clone();
        let tunnel = raw_tunnel
            .build()
            .map_err(|msg| anyhow::anyhow!("tunnel \"{name}\": {msg}"))?;
        tunnels.push(tunnel);
    }

    let mut names = HashSet::new();
    let mut ports = HashSet::new();
    for t in &tunnels {
        ensure!(
            names.insert(t.name.as_str()),
            "tunnel \"{}\": nome già usato da un altro tunnel",
            t.name
        );
        ensure!(
            ports.insert(t.local_port),
            "tunnel \"{}\": `local_port` = {} già usata da un altro tunnel",
            t.name,
            t.local_port
        );
    }

    Ok(Config {
        settings: raw.settings,
        tunnels,
    })
}

pub fn parse(text: &str) -> Result<Config> {
    let raw: RawConfig = toml::from_str(text).context("TOML non valido")?;
    validate(raw)
}

/// `~/.config/db-tunnel/config.toml` su Linux.
pub fn default_path() -> Result<PathBuf> {
    let dir = dirs::config_dir().context("cartella di configurazione non trovata")?;
    Ok(dir.join("db-tunnel").join("config.toml"))
}

pub fn load(path: &Path) -> Result<Config> {
    // Lettura sincrona: file piccolo, letto una sola volta all'avvio.
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("impossibile leggere {}", path.display()))?;
    parse(&text).with_context(|| format!("errore in {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC_EXAMPLE: &str = r#"
[settings]
check_interval_secs = 10
failure_threshold = 3
startup_timeout_secs = 20

[[tunnel]]
name = "prod-pg"
engine = "postgres"
mode = "ssh"
project = "my-project"
zone = "europe-west8-a"
instance = "db-vm-1"
local_port = 15432
remote_host = "127.0.0.1"
remote_port = 5432

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
instance_connection_name = "my-project:europe-west8:my-pg-db"
local_port = 15434
"#;

    /// Config con un solo tunnel, per i test che guardano un campo alla volta.
    fn one(body: &str) -> String {
        format!("[[tunnel]]\nname = \"t\"\nlocal_port = 15432\n{body}")
    }

    /// Messaggio di errore completo (con le cause concatenate).
    fn error_of(text: &str) -> String {
        format!("{:#}", parse(text).unwrap_err())
    }

    #[test]
    fn esempio_della_spec_valido() {
        let config = parse(SPEC_EXAMPLE).unwrap();
        assert_eq!(config.tunnels.len(), 3);
        assert_eq!(
            config.tunnels[0],
            Tunnel {
                name: "prod-pg".into(),
                engine: Engine::Postgres,
                local_port: 15432,
                target: Target::Ssh {
                    project: "my-project".into(),
                    zone: "europe-west8-a".into(),
                    instance: "db-vm-1".into(),
                    remote_host: "127.0.0.1".into(),
                    remote_port: 5432,
                },
            }
        );
        assert_eq!(
            config.tunnels[1].target,
            Target::Iap {
                project: "my-project".into(),
                zone: "europe-west8-b".into(),
                instance: "my-test-vm".into(),
                remote_port: 5432,
            }
        );
        assert_eq!(
            config.tunnels[2].target,
            Target::CloudSql {
                instance_connection_name: "my-project:europe-west8:my-pg-db".into(),
            }
        );
    }

    #[test]
    fn settings_di_default_se_assenti() {
        let config = parse(&one(
            "engine = \"postgres\"\nmode = \"cloudsql\"\ninstance_connection_name = \"p:r:i\"\n",
        ))
        .unwrap();
        assert_eq!(config.settings, Settings::default());
        assert_eq!(config.settings.check_interval_secs, 10);
        assert_eq!(config.settings.failure_threshold, 3);
        assert_eq!(config.settings.startup_timeout_secs, 20);
    }

    #[test]
    fn settings_personalizzate() {
        let text = format!(
            "[settings]\ncheck_interval_secs = 30\n{}",
            one(
                "engine = \"postgres\"\nmode = \"cloudsql\"\ninstance_connection_name = \"p:r:i\"\n"
            )
        );
        let config = parse(&text).unwrap();
        assert_eq!(config.settings.check_interval_secs, 30);
        // i campi non scritti restano al default
        assert_eq!(config.settings.failure_threshold, 3);
    }

    #[test]
    fn remote_port_di_default_per_engine() {
        let pg = parse(&one(
            "engine = \"postgres\"\nmode = \"iap\"\nproject = \"p\"\nzone = \"z\"\ninstance = \"i\"\n",
        ))
        .unwrap();
        let mysql = parse(&one(
            "engine = \"mysql\"\nmode = \"iap\"\nproject = \"p\"\nzone = \"z\"\ninstance = \"i\"\n",
        ))
        .unwrap();
        assert!(matches!(
            pg.tunnels[0].target,
            Target::Iap {
                remote_port: 5432,
                ..
            }
        ));
        assert!(matches!(
            mysql.tunnels[0].target,
            Target::Iap {
                remote_port: 3306,
                ..
            }
        ));
    }

    #[test]
    fn remote_host_di_default_con_ssh() {
        let config = parse(&one(
            "engine = \"postgres\"\nmode = \"ssh\"\nproject = \"p\"\nzone = \"z\"\ninstance = \"i\"\n",
        ))
        .unwrap();
        match &config.tunnels[0].target {
            Target::Ssh { remote_host, .. } => assert_eq!(remote_host, "127.0.0.1"),
            altro => panic!("atteso Ssh, trovato {altro:?}"),
        }
    }

    #[test]
    fn mode_mancante() {
        let err = error_of(&one("engine = \"postgres\"\n"));
        assert!(err.contains("tunnel \"t\""), "{err}");
        assert!(err.contains("`mode`"), "{err}");
    }

    #[test]
    fn nessun_tunnel() {
        let err = error_of("[settings]\ncheck_interval_secs = 5\n");
        assert!(err.contains("nessun tunnel"), "{err}");
    }

    #[test]
    fn nomi_duplicati() {
        let tunnel = |port: u16| {
            format!(
                "[[tunnel]]\nname = \"uguale\"\nengine = \"postgres\"\nmode = \"cloudsql\"\n\
                 instance_connection_name = \"p:r:i\"\nlocal_port = {port}\n"
            )
        };
        let err = error_of(&format!("{}{}", tunnel(15432), tunnel(15433)));
        assert!(err.contains("tunnel \"uguale\""), "{err}");
        assert!(err.contains("nome già usato"), "{err}");
    }

    #[test]
    fn porte_duplicate() {
        let tunnel = |name: &str| {
            format!(
                "[[tunnel]]\nname = \"{name}\"\nengine = \"postgres\"\nmode = \"cloudsql\"\n\
                 instance_connection_name = \"p:r:i\"\nlocal_port = 15432\n"
            )
        };
        let err = error_of(&format!("{}{}", tunnel("a"), tunnel("b")));
        assert!(err.contains("tunnel \"b\""), "{err}");
        assert!(err.contains("`local_port` = 15432"), "{err}");
    }

    #[test]
    fn local_port_fuori_intervallo() {
        for bad in ["80", "1023", "65536", "-1"] {
            let text = format!(
                "[[tunnel]]\nname = \"t\"\nengine = \"postgres\"\nmode = \"cloudsql\"\n\
                 instance_connection_name = \"p:r:i\"\nlocal_port = {bad}\n"
            );
            let err = error_of(&text);
            assert!(err.contains("`local_port`"), "{bad}: {err}");
            assert!(err.contains("1024-65535"), "{bad}: {err}");
        }
    }

    #[test]
    fn remote_host_non_ammesso_con_iap() {
        let err = error_of(&one(
            "engine = \"postgres\"\nmode = \"iap\"\nproject = \"p\"\nzone = \"z\"\ninstance = \"i\"\n\
             remote_host = \"10.0.0.1\"\n",
        ));
        assert!(err.contains("tunnel \"t\""), "{err}");
        assert!(err.contains("`remote_host` non è ammesso"), "{err}");
    }

    #[test]
    fn campo_obbligatorio_mancante() {
        let err = error_of(&one(
            "engine = \"postgres\"\nmode = \"ssh\"\nproject = \"p\"\ninstance = \"i\"\n",
        ));
        assert!(err.contains("`zone` è obbligatorio"), "{err}");
    }

    #[test]
    fn campi_vietati_con_cloudsql() {
        for (field, value) in [
            ("project", "\"p\""),
            ("zone", "\"z\""),
            ("instance", "\"i\""),
            ("remote_host", "\"127.0.0.1\""),
            ("remote_port", "5432"),
        ] {
            let err = error_of(&one(&format!(
                "engine = \"postgres\"\nmode = \"cloudsql\"\ninstance_connection_name = \"p:r:i\"\n\
                 {field} = {value}\n"
            )));
            assert!(
                err.contains(&format!("`{field}` non è ammesso")),
                "{field}: {err}"
            );
        }
    }

    #[test]
    fn instance_connection_name_non_ammesso_con_ssh() {
        let err = error_of(&one(
            "engine = \"postgres\"\nmode = \"ssh\"\nproject = \"p\"\nzone = \"z\"\ninstance = \"i\"\n\
             instance_connection_name = \"p:r:i\"\n",
        ));
        assert!(
            err.contains("`instance_connection_name` non è ammesso"),
            "{err}"
        );
    }

    #[test]
    fn instance_connection_name_malformato() {
        for bad in ["solo-nome", "a:b", "a:b:c:d", "a::c", ":b:c"] {
            let err = error_of(&one(&format!(
                "engine = \"postgres\"\nmode = \"cloudsql\"\ninstance_connection_name = \"{bad}\"\n"
            )));
            assert!(err.contains("progetto:regione:istanza"), "{bad}: {err}");
        }
    }

    #[test]
    fn campo_sconosciuto() {
        let err = error_of(&one(
            "engine = \"postgres\"\nmode = \"cloudsql\"\ninstance_connection_name = \"p:r:i\"\n\
             remote_hots = \"x\"\n",
        ));
        assert!(err.contains("remote_hots"), "{err}");
    }

    #[test]
    fn engine_sconosciuto() {
        let err = error_of(&one(
            "engine = \"oracle\"\nmode = \"cloudsql\"\ninstance_connection_name = \"p:r:i\"\n",
        ));
        assert!(err.contains("oracle"), "{err}");
    }
}
