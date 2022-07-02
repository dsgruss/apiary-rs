use core::str::FromStr;
use ipnet::Ipv4Net;
use local_ip_address::list_afinet_netifas;
use std::net::IpAddr::V4;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};

use crate::{Error, Network, PATCH_EP};

pub struct NativeInterface {
    socket: UdpSocket,
}

impl NativeInterface {
    pub fn new() -> Self {
        let ips = list_afinet_netifas().unwrap();
        let preferred_subnet: Ipv4Net = "10.0.0.0/8".parse().unwrap();
        let mut local_addr = Ipv4Addr::UNSPECIFIED;
        for (name, ip) in ips {
            match ip {
                V4(addr) => {
                    info!("IP Address: {:?} {:?}", name, addr);
                    if preferred_subnet.contains(&addr) {
                        local_addr = addr;
                    }
                }
                _ => {}
            }
        }
        info!("Using local address {:?}", local_addr);
        let patch_ep = SocketAddrV4::from_str(PATCH_EP).unwrap();
        let socket = UdpSocket::bind(SocketAddr::from((local_addr, patch_ep.port()))).unwrap();
        socket
            .join_multicast_v4(patch_ep.ip(), &local_addr)
            .unwrap();
        NativeInterface { socket }
    }
}

impl Network for NativeInterface {
    fn can_send(&mut self) -> bool {
        true
    }

    fn recv_directive(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        match self.socket.recv_from(buf) {
            Ok((size, _)) => Ok(size),
            Err(_) => Err(Error::NoData),
        }
    }

    fn send_directive(&mut self, buf: &[u8]) -> Result<(), Error> {
        match self.socket.send_to(buf, PATCH_EP) {
            Ok(_) => Ok(()),
            Err(_) => Err(Error::Network),
        }
    }

    fn jack_connect(
        &mut self,
        jack_id: u32,
        addr: &str,
        port: u16,
        time: i64,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn jack_recv(&mut self, jack_id: u32, buf: &mut [u8]) -> Result<usize, Error> {
        Ok(0)
    }

    fn jack_send(&mut self, jack_id: u32, buf: &[u8]) -> Result<(), Error> {
        Ok(())
    }
}
