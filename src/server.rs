use std::collections::HashMap;
use std::sync::Mutex;
use std::fs;
use std::thread;

mod udp;

pub fn start(port: String, location: String) {
    let mut hosts: Vec<udp::Host> = Vec::new();
    if !location.is_empty() {
        read_hosts(&mut hosts, &location);
    }
    let h = udp::Header {
        request: "disc".to_string(),
        dest_port: 8000,
        src_port: 8080,
        dest_ip: "127.0.0.1".to_string(),
        src_ip: "127.0.0.1".to_string()
    };
    let table: HashMap<String, String> = HashMap::new();
    let rtable = Mutex::new(table);
    let connection = udp::Server::init(&port, rtable, &mut hosts);
    connection.send_discovery(h);
    let (process_handler, listen_handler) = connection.listen();
    process_handler.join().unwrap();
    listen_handler.join().unwrap();
}

fn read_hosts(hosts: &mut Vec<udp::Host>, location: &str) {
    let raw_hosts = fs::read_to_string(location)
        .expect("could not read hosts form file");
    let raw_hosts: Vec<&str> = raw_hosts.split("\n").collect();
    for raw_host in raw_hosts {
        let host:Vec<&str> = raw_host.split(" ").collect();
        let host = udp::Host::new(
        host[0].to_string(),
        host[1].to_string(),
        host[2].parse::<u16>().expect("not parsable")
        );
        hosts.push(host);
    }
}

fn start_discovery(connection: &udp::Server) {
    thread::sleep_ms(5000);
    for (i, host) in connection.hosts.iter().enumerate() {
    }
}
