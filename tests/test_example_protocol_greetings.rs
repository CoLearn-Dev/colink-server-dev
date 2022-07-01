use rand::Rng;
use std::{
    net::TcpStream,
    process::{Child, Command, Stdio},
};
use tracing::debug;

const CORE_ADDR: &str = "127.0.0.1";
const MQ_AMQP: &str = "amqp://guest:guest@localhost:5672";
const MQ_API: &str = "http://guest:guest@localhost:15672/api";
const MQ_PREFIX: &str = "colink-test";
const USER_NUM: [usize; 11] = [2, 2, 2, 2, 2, 3, 3, 4, 4, 5, 5];

struct KilledWhenDrop(Child);

impl Drop for KilledWhenDrop {
    fn drop(&mut self) {
        self.0.kill().unwrap()
    }
}

#[test]
fn test_main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    build();
    for i in 0..11 {
        test_greetings(12300 + i, USER_NUM[i as usize])?;
    }
    Ok(())
}

fn test_greetings(port: u16, user_num: usize) -> Result<(), Box<dyn std::error::Error>> {
    let addr = format!("http://{}:{}", CORE_ADDR, port);
    let mut child_processes = vec![];

    assert!(
        TcpStream::connect(&format!("{}:{}", CORE_ADDR, port)).is_err(),
        "listen {}:{}: address already in use.",
        CORE_ADDR,
        port
    );
    if std::fs::metadata("host_token.txt").is_ok() {
        std::fs::remove_file("host_token.txt")?;
    }
    child_processes.push(KilledWhenDrop(start_core(port)));
    loop {
        if std::fs::metadata("host_token.txt").is_ok()
            && TcpStream::connect(&format!("{}:{}", CORE_ADDR, port)).is_ok()
        {
            break;
        }
        std::thread::sleep(core::time::Duration::from_millis(100));
    }

    let host_token: String = String::from_utf8_lossy(&std::fs::read("host_token.txt")?).parse()?;
    let users = host_import_users_and_exchange_guest_jwts(&addr, &host_token, user_num);
    debug!("users:{:?}", users);
    assert!(users.len() == user_num);
    let start_time = chrono::Utc::now().timestamp_nanos();
    let random_number = rand::thread_rng().gen_range(0..1000);

    let mut threads = vec![];
    if user_num == 2 {
        threads.push({
            let users = users.clone();
            let time: u64 = rand::thread_rng().gen_range(0..1000);
            let msg = random_number.to_string();
            let addr = addr.clone();
            std::thread::spawn(move || {
                std::thread::sleep(core::time::Duration::from_millis(time));
                user_run_task(&addr, &users[0], &users[1], &msg)
            })
        });
    } else {
        threads.push({
            let users = users.clone();
            let time: u64 = rand::thread_rng().gen_range(0..1000);
            let addr = addr.clone();
            std::thread::spawn(move || {
                std::thread::sleep(core::time::Duration::from_millis(time));
                user_greetings_to_multiple_users(&addr, &users)
            })
        });
    }
    for i in 1..users.len() {
        threads.push({
            let users = users.clone();
            let time: u64 = rand::thread_rng().gen_range(0..1000);
            let addr = addr.clone();
            std::thread::spawn(move || {
                std::thread::sleep(core::time::Duration::from_millis(time));
                run_auto_confirm(&addr, &users[i], "greetings")
            })
        });
    }
    for user in &users {
        // TODO: set gen_range to (1..4) when we implement the blocker mentioned in https://github.com/camelop/dds-dev/issues/25
        let num: usize = rand::thread_rng().gen_range(1..2); // Generate the number of operators for testing multiple protocol operators.
        for _ in 0..num {
            threads.push({
                let user = user.clone();
                let time: u64 = rand::thread_rng().gen_range(0..1000);
                let addr = addr.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(core::time::Duration::from_millis(time));
                    run_protocol_greetings(&addr, &user)
                })
            });
        }
    }
    for join in threads {
        child_processes.push(KilledWhenDrop(join.join().unwrap()));
    }

    let rnd_receiver = rand::thread_rng().gen_range(1..user_num);
    let msg = get_next_greeting_message(&addr, &users[rnd_receiver], start_time);
    debug!("msg:{:?}", msg);
    if user_num == 2 {
        assert!(msg.parse::<i32>()? == random_number);
    } else {
        assert!(msg == "hello");
    }
    Ok(())
}

fn start_core(port: u16) -> Child {
    Command::new("cargo")
        .args([
            "run",
            "--",
            "--address",
            CORE_ADDR,
            "--port",
            &port.to_string(),
            "--mq-amqp",
            MQ_AMQP,
            "--mq-api",
            MQ_API,
            "--mq-prefix",
            MQ_PREFIX,
        ])
        .current_dir("./")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn build() {
    let mut core = Command::new("cargo")
        .args(["build", "--all-targets"])
        .current_dir("./")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut sdk_a = Command::new("cargo")
        .args(["build", "--all-targets"])
        .current_dir("./tests/sdk-a")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut sdk_p = Command::new("cargo")
        .args(["build", "--all-targets"])
        .current_dir("./tests/sdk-p")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    core.wait().unwrap();
    sdk_a.wait().unwrap();
    sdk_p.wait().unwrap();
}

fn host_import_users_and_exchange_guest_jwts(addr: &str, jwt: &str, num: usize) -> Vec<String> {
    let res = Command::new("cargo")
        .args([
            "run",
            "--example",
            "host_import_users_and_exchange_guest_jwts",
            addr,
            jwt,
            &num.to_string(),
        ])
        .current_dir("./tests/sdk-a")
        .output()
        .unwrap();
    debug!(
        "res:{:?}, {:?}, {}",
        res.stdout,
        String::from_utf8_lossy(&res.stderr).to_string(),
        res.status
    );
    let res = String::from_utf8_lossy(&res.stdout).to_string();
    debug!("res:{:?}", res);
    let users = res.split_whitespace().map(|s| s.to_string()).collect();
    users
}

fn user_run_task(addr: &str, jwt_a: &str, jwt_b: &str, msg: &str) -> Child {
    Command::new("cargo")
        .args(["run", "--example", "user_run_task", addr, jwt_a, jwt_b, msg])
        .current_dir("./tests/sdk-a")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn user_greetings_to_multiple_users(addr: &str, jwts: &[String]) -> Child {
    let mut args = vec!["run", "--example", "user_greetings_to_multiple_users", addr];
    for jwt in jwts {
        args.push(jwt);
    }
    Command::new("cargo")
        .args(&args)
        .current_dir("./tests/sdk-a")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn run_auto_confirm(addr: &str, jwt: &str, protocol_name: &str) -> Child {
    Command::new("cargo")
        .args(["run", "--example", "auto_confirm", addr, jwt, protocol_name])
        .current_dir("./tests/sdk-a")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn run_protocol_greetings(addr: &str, jwt: &str) -> Child {
    Command::new("cargo")
        .args([
            "run",
            "--example",
            "protocol_greetings",
            "--",
            "--addr",
            addr,
            "--jwt",
            jwt,
        ])
        .current_dir("./tests/sdk-p")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn get_next_greeting_message(addr: &str, jwt: &str, now: i64) -> String {
    let res = Command::new("cargo")
        .args([
            "run",
            "--example",
            "get_next_greeting_message",
            addr,
            jwt,
            &now.to_string(),
        ])
        .current_dir("./tests/sdk-a")
        .output()
        .unwrap();
    let mut msg = String::from_utf8_lossy(&res.stdout).to_string();
    msg.pop();
    msg
}
