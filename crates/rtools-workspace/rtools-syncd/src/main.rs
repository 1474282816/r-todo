use anyhow::Result;
use clap::{Parser, Subcommand};
use rand::RngCore;
use rtools_syncd::{
    db,
    http::{serve, AppState},
};
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::{broadcast, Mutex};

#[derive(Parser, Debug)]
#[command(name = "rtools-syncd", version)]
struct Cli {
    #[arg(
        long,
        global = true,
        default_value = "./data",
        env = "RTOOLS_DATA_DIR"
    )]
    data_dir: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Serve {
        #[arg(long, default_value = "0.0.0.0:18092")]
        bind: SocketAddr,
    },
    Pair {
        #[arg(long, default_value_t = 10)]
        minutes: i64,
    },
    Devices,
    Revoke {
        device_id: String,
    },
    Check,
    Backup {
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Seed {
        json: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rtools_syncd=info,tower_http=info".into()),
        )
        .init();
    let cli = Cli::parse();
    std::fs::create_dir_all(&cli.data_dir)?;
    let db_path = cli.data_dir.join("rtools-syncd.db");
    match cli.command {
        Command::Serve { bind } => {
            let conn = db::open(&db_path)?;
            tracing::info!(
                db=?db_path,
                integrity=%db::integrity_check(&conn)?,
                cursor=db::current_cursor(&conn)?,
                "database ready"
            );
            let (events, _) = broadcast::channel(256);
            let state = AppState {
                db_path,
                lock: Arc::new(Mutex::new(())),
                events,
            };
            serve(state, bind).await
        }
        Command::Pair { minutes } => {
            let conn = db::open(&db_path)?;
            let code = generate_code();
            db::create_pairing_code(&conn, &code, minutes)?;
            println!("pairing_code={code}");
            println!(
                "expires_at={}",
                (chrono::Utc::now() + chrono::Duration::minutes(minutes)).to_rfc3339()
            );
            Ok(())
        }
        Command::Devices => {
            let conn = db::open(&db_path)?;
            let mut stmt = conn.prepare(
                "SELECT id,name,created_at,last_seen_at,revoked_at FROM devices ORDER BY created_at",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(format!(
                    "{}\t{}\tcreated={}\tlast={}\trevoked={}",
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    r.get::<_, Option<String>>(4)?.unwrap_or_default()
                ))
            })?;
            for row in rows {
                println!("{}", row?);
            }
            Ok(())
        }
        Command::Revoke { device_id } => {
            let conn = db::open(&db_path)?;
            println!("revoked={}", db::revoke_device(&conn, &device_id)?);
            Ok(())
        }
        Command::Check => {
            let conn = db::open(&db_path)?;
            println!("integrity={}", db::integrity_check(&conn)?);
            println!("cursor={}", db::current_cursor(&conn)?);
            Ok(())
        }
        Command::Backup { output } => {
            let conn = db::open(&db_path)?;
            let output = output.unwrap_or_else(|| {
                cli.data_dir.join(format!(
                    "backups/rtools-syncd-{}.db",
                    chrono::Utc::now().format("%Y%m%d-%H%M%S")
                ))
            });
            db::backup(&conn, &output)?;
            println!("backup={}", output.display());
            Ok(())
        }
        Command::Seed { json } => {
            let mut conn = db::open(&db_path)?;
            let content = std::fs::read_to_string(&json)?;
            let tasks =
                rtools_domain::parse_rtodo_json(&content).map_err(anyhow::Error::msg)?;
            let mut count = 0;
            for task in tasks {
                if db::get_task(&conn, &task.id)?.is_none() {
                    rtools_syncd::service::seed_task(&mut conn, task)?;
                    count += 1;
                }
            }
            println!("seeded={count}");
            Ok(())
        }
    }
}

fn generate_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = [0u8; 8];
    rand::rng().fill_bytes(&mut bytes);
    let code: String = bytes
        .iter()
        .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
        .collect();
    format!("{}-{}", &code[..4], &code[4..])
}
