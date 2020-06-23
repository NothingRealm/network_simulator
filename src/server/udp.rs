use std::net::UdpSocket;
use std::io::prelude::*;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use std::thread;
use std::mem;
use std::time::Duration;
use std::fs;
use std::net::{TcpListener, TcpStream};
use crate::bytes;

const BUFFER_SIZE: usize = 8192;
const USEFUL_BUFFER_SIZE: usize = BUFFER_SIZE - 16;
const MAX_CONEECTION: u16 = 20;

pub struct Host {
    pub name: String,
    pub ipaddr: String,
    pub port: u16,
    pub num_requests: u16
}

pub struct Server {
    pub socket: UdpSocket,
    pub hosts: Arc<RwLock<HashMap<String, RwLock<Host>>>>,
    pub requests: Arc<RwLock<Vec<String>>>,
    pub udp_port: u16,
    pub ipaddr: String,
    pub connection_num: Arc<RwLock<u16>>,
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
    pub fn new(name: String, ipaddr: String, port: u16) -> RwLock<Host> {
        let num_requests = 0;
        let host = Host {
            name,
            ipaddr,
            port,
            num_requests
        };
        RwLock::new(host)
    }
}

impl Server {
    pub fn init(
        udp_port: &str,
        hosts: Arc<RwLock<HashMap<String, RwLock<Host>>>>,
        ipaddr: &str,
        requests: Arc<RwLock<Vec<String>>>,
        ) -> Server {
        let socket  = UdpSocket::bind(format!("127.0.0.1:{}", udp_port))
            .expect("Something went
                wrong while trying to create UDP socket!!");
        let udp_port = udp_port.parse::<u16>().expect("non parsable port");
        let ipaddr = ipaddr.to_string();
        let connection_num = Arc::new(RwLock::new(0));
        Server {
            socket,
            hosts,
            requests,
            udp_port,
            ipaddr,
            connection_num
        }
    }

    pub fn listen(self, dir: String) -> (thread::JoinHandle<u32>,
            thread::JoinHandle<u32>,
            thread::JoinHandle<u32>) {

        let myaddr = self.ipaddr.clone();
        let udp_p: u16 = self.udp_port.clone();
        let (tx, rx): (mpsc::Sender<(usize, [u8; BUFFER_SIZE])>,
        mpsc::Receiver<(usize, [u8; BUFFER_SIZE])>) = mpsc::channel();

        let discover_handler_hosts = self.hosts.clone();
        let discover_handler_soc = self.socket.try_clone()
            .expect("Could not clone");
        let discover_handler = thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_secs(10));
                Server::start_discovery(
                    discover_handler_hosts.clone(),
                    &myaddr, udp_p,
                    discover_handler_soc.try_clone().unwrap()
                    );
            }
        });

        let requests = self.requests.clone();
        let connection_num = self.connection_num.clone();
        let process_handler_hosts = self.hosts.clone();
        let process_handler_soc = self.socket.try_clone()
            .expect("Could not clone");
        let process_handler = thread::spawn(move || {
            loop {
                let (amt, data) = rx.recv().unwrap();
                let header = Server::extract_header(&data);
                let current = 16;
                let request:&str = &header.request.replace("\u{0}", "");
                println!("recived {:?}", request);
                match request {
                    "get" => {
                        Server::process_get(&data, current,
                            &header, &dir, &process_handler_soc,
                            connection_num.clone(), 
                            process_handler_hosts.clone());
                    },
                    "disc" => {
                        Server::discovery(process_handler_hosts.clone(),
                        &data, 16, amt);
                    },
                    "OK" => {
                        Server::process_ok(current, data, requests.clone(),
                            header, &dir);
                    },
                    _ => {
                        continue;
                    }
                }
            }
        });

        let listen_handler_soc = self.socket.try_clone()
            .expect("Could not clone");
        let listen_handler = thread::spawn(move || {
            loop {
                let mut buf = [32; BUFFER_SIZE];
                let (amt, _src) = listen_handler_soc.recv_from(&mut buf)
                    .expect("shit happened");
                tx.send((amt, buf)).unwrap();
            }
        });
        return (process_handler, listen_handler, discover_handler);
    }

    fn discovery(hosts: Arc<RwLock<HashMap<String, RwLock<Host>>>>,
        data: &[u8], current: usize, end: usize) {
        let mut current = current;
        let mut hosts = hosts.write().unwrap();
        while current < end { 
            let name_len = data[current];
            current += 1;
            let name = bytes::extract::extract_str(data,
                current, current + name_len as usize);
            current += name_len as usize;
            let ipaddr = bytes::extract::extract_ip(data, current);
            current += 4;
            let port = bytes::extract::extract_u16(data, current);
            current += 2;
            let key = format!("{}:{}", ipaddr, port);
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
            bytes::copy::copy_u16(buf, current, body_len);
            current += 2;
            bytes::copy::copy_str(buf, current, body);
            current += body_len;
            current as usize
    }

    pub fn get(socket: &UdpSocket, path: &str,
        hosts: Arc<RwLock<HashMap<String, RwLock<Host>>>>,
        src_port: u16, src_ip: &str, requests: Arc<RwLock<Vec<String>>>) {
        let mut _requests = requests.write().unwrap();
        _requests.push(path.to_string());
        let clear_request = requests.clone();
        let hosts = hosts.read().unwrap();
        for (_, host) in hosts.iter() {
            let host = host.read().unwrap();
            if host.ipaddr == src_ip && host.port == src_port {
                continue;
            }
            let mut buf: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
            let header = Header::new("get", host.port, src_port,
                &host.ipaddr, src_ip);
            let current = Server::create_file_packet(&mut buf, &header, path);
            Server::send(&socket, &host.ipaddr,
                host.port, buf, current as usize);
            println!("sending");
        }
        let path = path.to_string();
        let _ = thread::spawn(move || {
            thread::sleep(Duration::from_secs(10));
            let mut req = clear_request.write().unwrap();
            let index = req.iter().position(|x| *x == path);
            match index {
                Some(index) => {
                    req.remove(index);
                    println!("Could not find");
                },
                None => {
                }
            };
        });
    }

    pub fn send_discovery(socket: &UdpSocket,
        hosts: Arc<RwLock<HashMap<String, RwLock<Host>>>>,
        header: Header) {
        let mut counter = 0;
        let mut flag = true;
        let hosts = hosts.read().unwrap();
        let hosts: Vec<&RwLock<Host>> =
            hosts.iter().map(|(_, host)| host).collect();
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
                let host = hosts[i].read().unwrap();
                current = Server::copy_discovery_data(&mut buf,
                    current,
                    &host.name,
                    &host.ipaddr,
                    host.port);
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

    fn copy_discovery_data(buf: &mut [u8; BUFFER_SIZE],
        current: u16,
        name: &str,
        ipaddr: &str,
        port: u16) -> u16 {
        let name_len = name.len() as u8;
        let mut current = current;
        buf[current as usize] = name_len;
        current += 1;
        bytes::copy::copy_str(buf, current, name);
        current += name_len as u16;
        bytes::copy::copy_ip(buf, current, ipaddr);
        current += 4;
        bytes::copy::copy_u16(buf, current, port);
        current += 2;
        current
    }

    fn copy_header(buf: &mut [u8], header: &Header) -> u16 {
            bytes::copy::copy_str(buf, 0, &header.request);
            bytes::copy::copy_u16(buf, 4, header.dest_port);
            bytes::copy::copy_u16(buf, 6, header.src_port);
            bytes::copy::copy_ip(buf, 8, &header.dest_ip);
            bytes::copy::copy_ip(buf, 12, &header.src_ip);
            return 16;
    }

    fn extract_header(data: &[u8]) -> Header {
        let request = bytes::extract::extract_str(&data, 0, 4).trim()
            .to_string(); 
        let dest_port = bytes::extract::extract_u16(&data, 4);
        let src_port = bytes::extract::extract_u16(&data, 6);
        let dest_ip = bytes::extract::extract_ip(&data, 8);
        let src_ip = bytes::extract::extract_ip(&data, 12);
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

    fn process_ok(current: usize,
        data: [u8; BUFFER_SIZE],
        requests: Arc<RwLock<Vec<String>>>,
        header: Header,
        dir: &str) {
        let mut current = current;
        let file_len = bytes::extract::extract_u16(&data,
            current) as usize;
        current += 2;
        let file = bytes::extract::extract_str(&data, current,
            current + file_len);
        current += file_len;
        let mut requests = requests.write().unwrap();
        let index = requests.iter().position(|x| *x == file);
        match index {
            Some(index) => {
                requests.remove(index);
            },
            None => {
                return;
            }
        };
        let tcp_port = bytes::extract::extract_u16(&data,
            current);
        current += 2;
        let buffer_size = bytes::extract::extract_u16(&data,
            current);
        println!("buffer_size is: {}", buffer_size);
        let mut buf  = vec![0 as u8; buffer_size as usize];
        let addr = format!("{}:{}", header.src_ip, tcp_port);
        let mut tcp_connection =
            TcpStream::connect(addr).unwrap();
        let location = format!("./{}/{}", dir, file);
        let mut f = fs::File::create(location).unwrap();
        thread::spawn(move || {
            while tcp_connection.read(&mut buf).unwrap() != 0 {
                f.write(&mut buf)
                    .expect("Could not write file");
            }
        });
    }

    fn process_get(data: &[u8], current: usize, header: &Header, dir: &str,
        socket: &UdpSocket, connection_num: Arc<RwLock<u16>>, 
        hosts: Arc<RwLock<HashMap<String, RwLock<Host>>>>) {
        let mut current = current;
        let num_requests: u16;
        {
            let key = format!("{}:{}", header.src_ip, header.src_port);
            let hosts = hosts.write().unwrap();
            match hosts.get(&key) {
                Some(host) => {
                    let mut host = host.write().unwrap();
                    host.num_requests += 1;
                    num_requests = host.num_requests;
                },
                None => {
                    return;
                }
            }
        }
        let req_file_len = bytes::extract::extract_u16(&data, current);
        current += 2;
        let req_file = bytes::extract::extract_str(&data,
            current, current + req_file_len as usize);
        if Server::find_file(req_file, &dir) {
            let mut buf: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
            let resph = Header::new("OK", header.src_port,
                header.dest_port, 
                &header.src_ip, &header.dest_ip);
            let mut current 
                = Server::create_file_packet(&mut buf,
                &resph, req_file);
            let addr = format!("{}:{}", resph.src_ip, 0);
            let listener = TcpListener::bind(addr).unwrap();
            let socket_addr = listener.local_addr().unwrap();
            let port = socket_addr.port();
            let file = req_file.to_string();
            let directory = dir.to_string();
            let buffer_size: u16; 
            {
                let mut num = connection_num.write().unwrap();
                if *num > MAX_CONEECTION  {
                    return;
                }
                *num += 1;
                buffer_size = Server::calculate_buffer(*num, num_requests);
            }
            thread::spawn(move || {
                match listener.accept() {
                    Ok((mut socket, _addr)) => {
                        socket.set_nodelay(true)
                            .expect("Could not set no delay");
                        let mut buffer 
                            = vec![0 as u8; 2048];
                        let location = format!("./{}/{}", directory, file);
                        let mut f = fs::File::open(location)
                            .expect("Could not open file");
                        while f.read(&mut buffer).unwrap() != 0 {
                            socket.write(&buffer).unwrap();
                        }
                    },
                    Err(e) => println!("couldn't get client: {:?}", e)
                }
                let mut num = connection_num.write().unwrap();
                *num -= 1;
            });
            bytes::copy::copy_u16(&mut buf, current as u16, port);
            current += 2;
            bytes::copy::copy_u16(&mut buf, current as u16, buffer_size);
            current += 2;
            Server::send(&socket, &header.src_ip,
                header.src_port, buf, current);
        }
    }

    fn start_discovery(hosts: Arc<RwLock<HashMap<String, RwLock<Host>>>>, 
        myaddr: &str, udp_p: u16, socket: UdpSocket) {
        let _hosts = hosts.read().unwrap();
        for (_, host) in _hosts.iter() {
            let host = host.read().unwrap();
            if host.ipaddr == myaddr && host.port == udp_p {
                continue
            }
            let header = Header::new("disc",
                host.port, udp_p, &host.ipaddr, &myaddr);
            Server::send_discovery(&socket, hosts.clone(), header);
        }
    }

    fn calculate_buffer(cons_num: u16, num_requests: u16) -> u16 {
        BUFFER_SIZE as u16 / (cons_num + num_requests / 4)
    }

}
