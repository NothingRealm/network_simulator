use std::net::UdpSocket;
use std::io;
use std::io::prelude::*;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Mutex, Arc, RwLock};
use std::thread;
use std::mem;
use std::time::Duration;
use std::fs;
use std::net::TcpListener;

const BUFFER_SIZE: usize = 2048;
const USEFUL_BUFFER_SIZE: usize = BUFFER_SIZE - 16;

pub struct Host {
    pub name: String,
    pub ipaddr: String,
    pub port: u16
}

pub struct Server {
    pub socket: UdpSocket,
    pub hosts: Arc<RwLock<HashMap<String, Host>>> ,
    rtable: Mutex<HashMap<String, String>>,
    pub udp_port: u16,
    pub ipaddr: String
}

pub struct Header {
    pub request: String,
    pub dest_port: u16,
    pub src_port: u16,
    pub dest_ip: String,
    pub src_ip: String
}


impl Header {
    pub fn new(request: &str, dest_port: u16,
            src_port: u16, dest_ip: &str, src_ip: &str) -> Header {
        let request = request.to_string();
        let dest_ip = dest_ip.to_string();
        let src_ip = src_ip.to_string();
        Header {
            request,
            dest_port,
            src_port,
            dest_ip,
            src_ip
        }
    }
}

impl Host {
    pub fn new(name: String, ipaddr: String, port: u16) -> Host {
        Host {
            name,
            ipaddr,
            port
        }
    }
}

impl Server {
    pub fn init(
        udp_port: &str,
        rtable: Mutex<HashMap<String, String>>,
        hosts: Arc<RwLock<HashMap<String, Host>>>,
        ipaddr: &str
        ) -> Server {
        let socket  = UdpSocket::bind(format!("127.0.0.1:{}", udp_port))
            .expect("Something went
                wrong while trying to create UDP socket!!");
        let udp_port = udp_port.parse::<u16>().expect("non parsable port");
        let ipaddr = ipaddr.to_string();
        Server {
            socket,
            hosts,
            rtable,
            udp_port,
            ipaddr
        }
    }

    pub fn listen(self, dir: String)
        -> (thread::JoinHandle<u32>, thread::JoinHandle<u32>) {
        let soc = self.socket.try_clone().expect("Could not clone");
        let soc2 = self.socket.try_clone().expect("Could not clone");
        let soc3 = self.socket.try_clone().expect("Could not clone");
        let (tx, rx): (mpsc::Sender<(usize, [u8; BUFFER_SIZE])>,
        mpsc::Receiver<(usize, [u8; BUFFER_SIZE])>) = mpsc::channel();
        let x = self.hosts.clone();
        let z = self.hosts.clone();
        let y = self.hosts.clone();
        let myaddr = self.ipaddr.clone();
        let udp_p: u16 = self.udp_port.clone();
        let _discover_handler = thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_secs(10));
                let hosts = z.read().unwrap();
                for (_, host) in hosts.iter() {
                    if host.ipaddr == myaddr && host.port == udp_p {
                        continue
                    }
                    let header = Header::new("disc",
                        host.port, udp_p, &host.ipaddr, &myaddr);
                    Server::send_discovery(&soc2, x.clone(), header);
                }
            }
        });
        let process_handler = thread::spawn(move || {
            loop {
                let (amt, data) = rx.recv().unwrap();
                let header = Server::extract_header(&data);
                let mut current = 16;
                let request:&str = &header.request.replace("\u{0}", "");
                println!("recived {:?}", request);
                match request {
                    "get" => {
                        Server::process_get(&data, current,
                            &header, &dir, &soc3);
                    },
                    "disc" => {
                        Server::discovery(y.clone(), &data, 16, amt);
                    },
                    "OK" => {
                        let file_len = extract_u16(&data, current) as usize;
                        current += 2;
                        let file = extract_str(&data, current,
                            current + file_len);
                        current += file_len;
                        let tcp_port = extract_u16(&data, current);
                        let mut buf: [u8; 2048] = [0; 2048];
                        let req_header = Header::new("star",
                            header.src_port, header.dest_port,
                            &header.src_ip, &header.dest_ip);
                        current = Server::create_file_packet(&mut buf,
                            &req_header, file); 
                        Server::send(&soc3, &req_header.dest_ip,
                            req_header.dest_port, buf, current);
                    },
                    "star" => {
                        println!("START");
                    },
                    _ => {
                        continue;
                    }
                }
            }
        });
        let listen_handler = thread::spawn(move || {
            loop {
                let mut buf = [32; BUFFER_SIZE];
                let (amt, _src) = soc.recv_from(&mut buf)
                    .expect("shit happened");
                tx.send((amt, buf)).unwrap();
            }
        });
        return (process_handler, listen_handler);
    }

    fn discovery(hosts: Arc<RwLock<HashMap<String, Host>>>,
        data: &[u8], current: usize, end: usize) {
        let mut current = current;
        let mut hosts = hosts.write().unwrap();
        while current < end { 
            let name_len = data[current];
            current += 1;
            let name = extract_str(data, current, current + name_len as usize);
            current += name_len as usize;
            let ipaddr = extract_ip(data, current);
            current += 4;
            let port = extract_u16(data, current);
            current += 2;
            println!("{}", name_len);
            println!("{:?}", name);
            println!("{:?}", ipaddr);
            println!("{}", port);
            let key = format!("{}:{}", ipaddr, port);
            println!("{:?}", key);
            if !hosts.contains_key(&key) {
                let host = Host::new(name.to_string(), ipaddr, port);
                hosts.insert(key, host); 
            }
        }
    }

    fn create_file_packet(buf: &mut [u8], header: &Header,
        body: &str) -> usize {
            let mut current = Server::copy_header(buf, &header);
            let body_len = body.len() as u16;
            copy_u16(buf, current, body_len);
            current += 2;
            copy_str(buf, current, body);
            current += body_len;
            current as usize
    }

    pub fn get(socket: &UdpSocket, path: &str,
        hosts: Arc<RwLock<HashMap<String, Host>>>,
        src_port: u16, src_ip: &str) {
        let hosts = hosts.read().unwrap();
        for (_, host) in hosts.iter() {
            let mut buf: [u8; 2048] = [0; 2048];
            let header = Header::new("get", host.port, src_port,
                &host.ipaddr, src_ip);
            let current = Server::create_file_packet(&mut buf, &header, path);
            Server::send(&socket, &host.ipaddr,
                host.port, buf, current as usize);
            println!("sending");
        }
    }

    pub fn send_discovery(socket: &UdpSocket,
        hosts: Arc<RwLock<HashMap<String, Host>>>, header: Header) {
        let mut counter = 0;
        let mut flag = true;
        let hosts = hosts.read().unwrap();
        let hosts: Vec<&Host> = hosts.iter().map(|(_, host)| host).collect();
        while flag {
            let mut buf: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE]; 
            let mut current: u16 = Server::copy_header(&mut buf, &header);
            let mut remained_buffer: i32 = USEFUL_BUFFER_SIZE as i32;
            for i in counter..hosts.len() {
                remained_buffer -= mem::size_of::<Host>() as i32;
                if remained_buffer < 0 {
                    counter = i;
                    break;
                }
                if i == hosts.len() - 1 {
                    flag = false;
                }
                let name_len = hosts[i].name.len() as u8;
                buf[current as usize] = name_len;
                current += 1;
                copy_str(&mut buf, current, &hosts[i].name);
                current += name_len as u16;
                copy_ip(&mut buf, current, &hosts[i].ipaddr);
                current += 4;
                copy_u16(&mut buf, current, hosts[i].port);
                current += 2;
            }
            Server::send(&socket, &header.dest_ip, header.dest_port,
                buf, current as usize);
        }
    }

    pub fn send(socket: &UdpSocket,
        ipaddr: &str,port: u16, buf: [u8; BUFFER_SIZE], amt: usize) {
        let ip = format!("{}:{}", ipaddr, port);
        socket.send_to(&buf[0..amt], ip)
            .expect("Could not send");
    }

    fn copy_header(buf: &mut [u8], header: &Header) -> u16 {
            copy_str(buf, 0, &header.request);
            copy_u16(buf, 4, header.dest_port);
            copy_u16(buf, 6, header.src_port);
            copy_ip(buf, 8, &header.dest_ip);
            copy_ip(buf, 12, &header.src_ip);
            return 16;
    }

    fn extract_header(data: &[u8]) -> Header {
        let request = extract_str(&data, 0, 4).trim().to_string(); 
        let dest_port = extract_u16(&data, 4);
        let src_port = extract_u16(&data, 6);
        let dest_ip = extract_ip(&data, 8);
        let src_ip = extract_ip(&data, 12);
        Header {
            request,
            dest_port,
            src_port,
            dest_ip,
            src_ip
        }
    }

    fn find_file(req: &str, dir: &str) -> bool {
        let files = fs::read_dir(&dir)
            .expect("could not read dir");
        for file in files {
            let req_file = file
                .expect("Could not read from dir")
                .path();
            let req_file = req_file.to_str().unwrap();
            let file: Vec<&str> = req_file
                .split("/")
                .collect();
            let file = file.last().unwrap();
            if *file == req {
                return true;
            }
        }
        return false;
    }

    fn process_get(data: &[u8], current: usize, header: &Header, dir: &str,
        socket: &UdpSocket) {
        let mut current = current;
        let req_file_len = extract_u16(&data, current);
        current += 2;
        let req_file = extract_str(&data,
            current, current + req_file_len as usize);
        if Server::find_file(req_file, &dir) {
            let mut buf: [u8; BUFFER_SIZE] = [0; 2048];
            let resph = Header::new("OK", header.src_port,
                header.dest_port, 
                &header.src_ip, &header.dest_ip);
            let mut current 
                = Server::create_file_packet(&mut buf,
                &resph, req_file);
            let addr = format!("{}:{}", resph.src_ip, 0);
            let listener = TcpListener::bind(addr).unwrap();
            let l = listener.try_clone();
            let socket_addr = listener.local_addr().unwrap();
            let port = socket_addr.port();
            let file = req_file.to_string();
            thread::spawn(move || {
                match listener.accept() {
                    Ok((mut socket, addr)) => {
                        let mut buffer: [u8; 2048] = [0; 2048];
                        let mut f = fs::File::open(file)
                            .expect("Could not open file");
                        while f.read(&mut buffer).unwrap() != 0 {
                            socket.write(&buffer).unwrap();
                        }
                    },
                    Err(e) => println!("couldn't get client: {:?}", e)
                }
            });
            copy_u16(&mut buf, current as u16, port);
            current += 2;
            Server::send(&socket, &header.src_ip,
                header.src_port, buf, current);
        }
    }

}

fn copy_str(buf: &mut [u8], current: u16, string: &str) {
    let string = string.as_bytes();
    for (i, byte) in string.iter().enumerate() {
        buf[current as usize + i] = *byte;
    }
}

fn copy_u16(buf: &mut [u8], current: u16, num: u16) {
    let num = num.to_be_bytes();
    buf[current as usize] = num[0];
    buf[current as usize + 1] = num[1];
}

fn copy_ip(buf: &mut [u8], current: u16, ip: &str) {
    let ip: Vec<&str> = ip.split(".").collect();
    for (i, num) in ip.iter().enumerate() {
        buf[current as usize + i] = num.parse::<u8>().expect("Wrong ip");
    }
}

fn extract_str(data: &[u8], start: usize, end: usize) -> &str {
    std::str::from_utf8(&data[start..end]).expect("Could not extract str")
}

fn extract_u16(data: &[u8], start: usize) -> u16 {
    ((data[start] as u16) << 8) + data[start + 1] as u16 
}

fn extract_ip(data: &[u8], start: usize) -> String {
    format!("{}.{}.{}.{}", data[start], data[start + 1],
        data[start + 2], data[start + 3])
}

