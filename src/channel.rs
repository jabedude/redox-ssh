use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::io::{FromRawFd, IntoRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{self, Stdio};
use std::thread::{self, JoinHandle};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc;
use packet::Packet;
use packet::WritePacketExt;
use sys;
use message::MessageType;

pub type ChannelId = u32;

#[derive(Debug)]
pub struct Channel {
    id: ChannelId,
    peer_id: ChannelId,
    process: Option<process::Child>,
    pty: Option<(RawFd, PathBuf)>,
    pub master: Option<File>,
    sender: Sender<Packet>,
    window_size: u32,
    peer_window_size: u32,
    max_packet_size: u32,
    read_thread: Option<JoinHandle<()>>,
}

// TODO: add the other reqs
#[derive(Debug)]
pub enum ChannelRequest {
    Pty {
        term: String,
        chars: u16,
        rows: u16,
        pixel_width: u16,
        pixel_height: u16,
        modes: Vec<u8>,
    },
    Shell,
    Env {
        var_name: String,
        var_value: String,
    },
}

impl Channel {
    pub fn new(
        id: ChannelId, peer_id: ChannelId, peer_window_size: u32,
        max_packet_size: u32, sender: Sender<Packet>,
    ) -> Channel {
        Channel {
            id: id,
            peer_id: peer_id,
            process: None,
            master: None,
            pty: None,
            sender: sender,
            window_size: peer_window_size,
            peer_window_size: peer_window_size,
            max_packet_size: max_packet_size,
            read_thread: None,
        }
    }

    pub fn id(&self) -> ChannelId {
        self.id
    }

    pub fn window_size(&self) -> u32 {
        self.window_size
    }

    pub fn max_packet_size(&self) -> u32 {
        self.max_packet_size
    }

    pub fn handle_request(&mut self, request: ChannelRequest) {
        match request
        {
            ChannelRequest::Pty {
                chars,
                rows,
                pixel_width,
                pixel_height,
                ..
            } => {
                let (master_fd, tty_path) = sys::getpty();

                sys::set_winsize(
                    master_fd,
                    chars,
                    rows,
                    pixel_width,
                    pixel_height,
                );
                let sender = self.sender.clone();
                let channel_id = self.id;

                self.read_thread = Some(thread::spawn( move || {
                    #[cfg(target_os = "redox")]
                    use syscall::dup;
                    #[cfg(target_os = "redox")]
                    let master2 = unsafe { dup(master_fd, &[]).unwrap_or(!0) };

                    #[cfg(not(target_os = "redox"))]
                    use libc::dup;
                    #[cfg(not(target_os = "redox"))]
                    let master2 = unsafe { dup(master_fd) };

                    println!("dup result: {}", master2 as u32);
                    let mut master = unsafe { File::from_raw_fd(master2) };
                    loop {
                        use std::str::from_utf8_unchecked;
                        let mut buf = [0; 4096];
                        let count = master.read(&mut buf).expect("error reading from master fd");
                        if count == 0 {
                            break;
                        }

                        let mut test_resp = Packet::new(MessageType::ChannelData);
                        test_resp.write_uint32(channel_id);
                        test_resp.write_bytes(&buf);

                       debug!("queuing up {:?} in tx queue", test_resp);
                       sender.send(test_resp);
                        
                        println!("Read: {}", unsafe {
                            from_utf8_unchecked(&buf[0..count])
                        });
                    }

                    warn!("Quitting read thread.");
                }));

                self.pty = Some((master_fd, tty_path));
                self.master = Some(unsafe { File::from_raw_fd(master_fd) });
            }
            ChannelRequest::Shell => {
                if let Some(&(fd, ref tty_path)) = self.pty.as_ref() {
                    debug!("tty_path: {:?}", tty_path);
                    let stdin = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(&tty_path)
                        .expect("unable to open stdin")
                        .into_raw_fd();

                    let stdout = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(&tty_path)
                        .expect("unable to open stdout")
                        .into_raw_fd();

                    let stderr = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(&tty_path)
                        .expect("unable to open stderr")
                        .into_raw_fd();

                    // TODO: this might require root to login
                    process::Command::new("bash")
                        .stdin(unsafe { Stdio::from_raw_fd(stdin) })
                        .stdout(unsafe { Stdio::from_raw_fd(stdout) })
                        .stderr(unsafe { Stdio::from_raw_fd(stderr) })
                        .before_exec(|| sys::before_exec())
                        .spawn()
                        .expect("unable to login");
                }
            }
            _ => debug!("Unhandled request"),
        }
        debug!("Channel Request: {:?}", request);
    }

    pub fn data(&mut self, data: &[u8]) -> io::Result<Option<Vec<u8>>> {
        if let Some(ref mut master) = self.master {
            master.write_all(data)?;
            master.flush()?;

            //let mut buf = vec![0u8; 4096];
            //let count = master.read(&mut buf)?;
            //Ok(Some(buf))
            Ok(None)
        }
        else {
            Ok(None)
        }
    }
}
