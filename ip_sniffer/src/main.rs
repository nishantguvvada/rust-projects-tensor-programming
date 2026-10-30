use std::env;
use std::io::{self, Write};
use std::net::{IpAddr, TcpStream};
use std::str::FromStr;
use std::process;
use std::thread;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

const MAX: u16 = 65535;

struct Arguments {
    ipaddr: IpAddr,
    threads: u16,
}

impl Arguments {
    fn new(args: &[String]) -> Result<Arguments, &'static str> {
        if args.len() < 2 {
            return Err("Not enough arguments");
        } else if args.len() > 4 {
            return Err("Too many arguments");
        }

        let flag = args[1].clone();
        
        // Help validation with precise flag isolation
        if flag == "-h" || flag == "--help" {
            if args.len() == 2 {
                println!("Usage:\r\n  -j <threads> <ipaddr>   Select how many threads to use\r\n  -h or --help            Display this help message\r\n  <ipaddr>                Scan with default threads (4)");
                return Err("Help requested");
            } else {
                return Err("Too many arguments for help flag");
            }
        }

        // Scenario: Default threads via direct IP assignment
        if let Ok(ipaddr) = IpAddr::from_str(&flag) {
            if args.len() > 2 {
                return Err("Too many arguments for default thread mode");
            }
            return Ok(Arguments { ipaddr, threads: 4 });
        }

        // Scenario: Targeted thread override flag
        if flag == "-j" {
            if args.len() < 4 {
                return Err("Missing parameters. Expected: -j <threads> <ipaddr>");
            }
            
            let threads = match args[2].parse::<u16>() {
                Ok(t) if t > 0 => t,
                Ok(_) => return Err("Number of threads must be greater than 0"),
                Err(_) => return Err("Invalid number of threads"),
            };

            let ipaddr = match IpAddr::from_str(&args[3]) {
                Ok(ip) => ip,
                Err(_) => return Err("Invalid IP address"),
            };

            return Ok(Arguments { ipaddr, threads });
        }

        Err("Invalid syntax")
    }
}

fn scan(tx: Sender<u16>, start_port: u16, addr: IpAddr, num_threads: u16) {
    let mut port = start_port; // Start precisely at the allocated thread chunk offset
    loop {
        // Use a short timeout connection so the scanner doesn't hang indefinitely 
        if TcpStream::connect_timeout(&(addr, port).into(), Duration::from_millis(500)).is_ok() {
            print!(".");
            io::stdout().flush().unwrap();
            if tx.send(port).is_err() {
                break; // Receiver disconnected, terminate thread execution
            }
        }

        if MAX - port < num_threads {
            break;
        }
        port += num_threads;
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();
    
    let arguments = Arguments::new(&args).unwrap_or_else(|err| {
        if err.contains("Help requested") {
            process::exit(0);
        } else {
            eprintln!("Program: {} Error: {}", program, err);
            process::exit(1);
        }
    });

    let num_threads = arguments.threads;
    let addr = arguments.ipaddr;
    let (tx, rx) = channel();

    println!("Scanning {} using {} threads...", addr, num_threads);

    for i in 0..num_threads {
        let tx_scan = tx.clone();
        thread::spawn(move || {
            scan(tx_scan, i, addr, num_threads);
            // Extraneous thread index signaling removed here to avoid data clutter
        });
    }

    // Drop primary transmitter so channel loops stop cleanly when children close
    drop(tx);

    let mut open_ports = vec![];
    for port in rx {
        open_ports.push(port);
    }

    println!("\nScan finished.");
    open_ports.sort();
    for port in open_ports {
        println!("Open port: {}", port);
    }
}
