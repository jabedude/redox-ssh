extern crate ssh;
extern crate log;

use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::process;
use std::str::FromStr;

use log::{LogLevelFilter, LogMetadata, LogRecord};

use ssh::{Server, ServerConfig};
use ssh::public_key::ED25519;

struct StdErrLogger;

impl log::Log for StdErrLogger {
    fn enabled(&self, _: &LogMetadata) -> bool {
        true
    }

    fn log(&self, record: &LogRecord) {
        if self.enabled(record.metadata()) {
            writeln!(io::stderr(), "{} - {}", record.level(), record.args())
                .unwrap();
        }
    }
}

pub fn main() {
    let mut verbosity = LogLevelFilter::Debug;
    let mut foreground = false;

    let key_pair = File::open("server.key").and_then(
        |mut f| (ED25519.import)(&mut f),
    );

    if let Some(ref err) = key_pair.as_ref().err() {
        writeln!(io::stderr(), "sshd: failed to open server.key: {}", err)
            .unwrap();
        process::exit(1);
    }

    let mut config = ServerConfig {
        host: String::from("0.0.0.0"),
        port: 22,
        key: key_pair.unwrap(),
    };

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_ref()
        {
            "-v" => verbosity = LogLevelFilter::Info,
            "-vv" => verbosity = LogLevelFilter::Debug,
            "-vvv" => verbosity = LogLevelFilter::Trace,
            "-f" => foreground = true,
            "-p" => {
                config.port =
                    u16::from_str(
                        &args.next().expect("sshd: no argument to -p option"),
                    ).expect("sshd: invalid port number to -p option");
            }
            _ => (),
        }
    }

    log::set_logger(|max_log_level| {
        max_log_level.set(verbosity);
        Box::new(StdErrLogger)
    }).unwrap();

    if !foreground {
        use ssh::sys::fork;
        if fork() != 0 {
            process::exit(0);
        }
    }

    let server = Server::with_config(config);

    if let Err(err) = server.run() {
        writeln!(io::stderr(), "sshd: {}", err).unwrap();
        process::exit(1);
    }
}
