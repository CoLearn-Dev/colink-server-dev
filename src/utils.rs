use crate::service::utils::{download_tgz, get_colink_home};
use rand::Rng;
use std::{
    path::Path,
    process::{Child, Command, Stdio},
};

pub struct RedisServer {
    pub process: Option<Child>,
}

impl Drop for RedisServer {
    fn drop(&mut self) {
        if self.process.is_some() {
            self.process.as_mut().unwrap().kill().unwrap();
        }
    }
}

pub async fn start_redis_server() -> Result<(RedisServer, String), Box<dyn std::error::Error>> {
    let mut port = rand::thread_rng().gen_range(10000..20000);
    while std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
        port = rand::thread_rng().gen_range(10000..20000);
    }
    let pg = passwords::PasswordGenerator::new()
        .length(32)
        .numbers(true)
        .lowercase_letters(true)
        .uppercase_letters(true);
    let password = pg.generate_one()?;
    let colink_home = get_colink_home()?;
    let redis_home = Path::new(&colink_home).join("redis-server");
    let program = Path::new(&redis_home).join("redis-server");
    if std::fs::metadata(program.clone()).is_err() {
        let base_url = "https://github.com/CoLearn-Dev/redis-binaries/releases/download/7.0.8";
        let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
        let (url, sha256) = match platform.as_str() {
            "linux-x86_64" => (
                format!("{}/redis-server-{}.tar.gz", base_url, platform),
                "5575cf43f41ef1bc9915667ca42822836e1c8f89f8bf338c2a9942617ba83714",
            ),
            "macos-x86_64" => (
                format!("{}/redis-server-{}.tar.gz", base_url, platform),
                "97c23a254283c259b764ad42ddd83eef4e138dbde057c7b862290bb283938a3b",
            ),
            _ => {
                return Err(format!(
                    "Cannot find the redis-server binary for platform {}.",
                    platform
                ))?;
            }
        };
        download_tgz(&url, sha256, redis_home.to_str().unwrap()).await?;
    }
    let process = Command::new(program)
        .args([
            "--port",
            &port.to_string(),
            "--requirepass",
            &password,
            "--save",
            "\"\"",
            "--appendonly",
            "no",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    loop {
        if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
            break;
        }
        std::thread::sleep(core::time::Duration::from_millis(10));
    }
    Ok((
        RedisServer {
            process: Some(process),
        },
        format!("redis://:{}@127.0.0.1:{}/", password, port),
    ))
}
