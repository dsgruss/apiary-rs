use core::mem::MaybeUninit;
use core::str::FromStr;
use ipnet::Ipv4Net;
use local_ip_address::list_afinet_netifas;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::IpAddr::V4;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use crate::{Error, Network, PATCH_EP};

pub struct NativeInterface {
    socket: Socket,
}

impl NativeInterface {
    pub fn new() -> Self {
        let ips = list_afinet_netifas().unwrap();
        let preferred_subnet: Ipv4Net = "10.0.0.0/8".parse().unwrap();
        let mut local_addr = Ipv4Addr::UNSPECIFIED;
        for (name, ip) in ips {
            match ip {
                V4(addr) => {
                    info!("Found IP address: {:?} {:?}", name, addr);
                    if preferred_subnet.contains(&addr) {
                        local_addr = addr;
                    }
                }
                _ => {}
            }
        }
        info!("Using local address {:?}", local_addr);
        let patch_ep = SocketAddrV4::from_str(PATCH_EP).unwrap();
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).unwrap();
        let address = SocketAddr::from((local_addr, patch_ep.port())).into();
        // The socket allows address reuse, which may be a security concern. However, we are
        // exclusively looking at UDP multicasts in this protocol.
        socket.set_reuse_address(true).unwrap();
        socket.set_nonblocking(true).unwrap();
        socket.bind(&address).unwrap();
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
        // Safety: the `recv` implementation promises not to write uninitialised
        // bytes to the `buf`fer, so this casting is safe.
        let buf = unsafe { &mut *(buf as *mut [u8] as *mut [MaybeUninit<u8>]) };
        match self.socket.recv_from(buf) {
            Ok((size, _)) => Ok(size),
            Err(_) => Err(Error::NoData),
        }
    }

    fn send_directive(&mut self, buf: &[u8]) -> Result<(), Error> {
        let patch_ep = SocketAddr::from_str(PATCH_EP).unwrap().into();
        match self.socket.send_to(buf, &patch_ep) {
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
