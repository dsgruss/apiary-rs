/*! Native socket interface.

This module provides communication (via the `Network` trait) using the native socket interface within the host operating system.
*/

use core::mem::MaybeUninit;
use core::str::FromStr;
use ipnet::Ipv4Net;
use local_ip_address::list_afinet_netifas;
use rand::{thread_rng, Rng};
use socket2::{Domain, Protocol, Socket, Type};
use std::io;
use std::net::IpAddr::V4;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use crate::{Error, Network, JACK_PORT, PATCH_EP, PREFERRED_SUBNET};

impl From<local_ip_address::Error> for Error {
    fn from(_: local_ip_address::Error) -> Self {
        Error::Network
    }
}

impl From<ipnet::AddrParseError> for Error {
    fn from(_: ipnet::AddrParseError) -> Self {
        Error::Parse
    }
}

impl From<std::net::AddrParseError> for Error {
    fn from(_: std::net::AddrParseError) -> Self {
        Error::Parse
    }
}

impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Error::Network
    }
}

pub struct NativeInterface<const I: usize, const O: usize> {
    patch_socket: Socket,
    patch_ep: SocketAddrV4,
    input_sockets: Vec<Socket>,
    input_groups: Vec<Option<Ipv4Addr>>,
    output_eps: Vec<SocketAddrV4>,
    local_addr: Ipv4Addr,
    input_buffers: [[u8; 1500]; I],
    output_buffer: [u8; 10000],
    enq_size: usize,
}

impl<const I: usize, const O: usize> NativeInterface<I, O> {
    pub fn new() -> Result<Self, Error> {
        let ips = list_afinet_netifas()?;
        let preferred_subnet: Ipv4Net = PREFERRED_SUBNET.parse()?;
        let mut local_addr = Ipv4Addr::UNSPECIFIED;
        for (name, ip) in ips {
            if let V4(addr) = ip {
                info!("Found IP address: {:?} {:?}", name, addr);
                if preferred_subnet.contains(&addr) {
                    local_addr = addr;
                }
            }
        }
        info!("Using local address {:?}", local_addr);

        let patch_ep = SocketAddrV4::from_str(PATCH_EP)?;
        let patch_socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        let address = SocketAddr::from((local_addr, patch_ep.port())).into();

        // The socket allows address reuse, which may be a security concern. However, we are
        // exclusively looking at UDP multicasts in this protocol.
        patch_socket.set_reuse_address(true)?;
        patch_socket.set_nonblocking(true)?;
        patch_socket.bind(&address)?;
        patch_socket.join_multicast_v4(patch_ep.ip(), &local_addr)?;

        let mut input_sockets = vec![];
        for _ in 0..I {
            let input_socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
            input_socket.set_reuse_address(true)?;
            input_socket.set_nonblocking(true)?;
            let input_address = SocketAddr::from((local_addr, JACK_PORT)).into();
            input_socket.bind(&input_address)?;
            input_sockets.push(input_socket);
        }

        // For now we just pick a random address in the multicast range for local testing purposes,
        // but ideally this will likely be some function of the interface address for devices that
        // all have their own ip (for instance, 10.0.42.69 => 239.42.69.(1,2, ...)). Source-specific
        // multicast could help here.
        let mut output_eps = vec![];
        let mut rng = thread_rng();
        for _ in 0..O {
            let addr = Ipv4Addr::new(
                239,
                rng.gen_range(0..255),
                rng.gen_range(0..255),
                rng.gen_range(0..255),
            );
            let ep = SocketAddrV4::new(addr, JACK_PORT);
            patch_socket.join_multicast_v4(&addr, &local_addr)?;
            info!("Jack endpoint: {:?}", ep);
            output_eps.push(ep);
        }

        Ok(NativeInterface {
            patch_socket,
            patch_ep,
            input_sockets,
            input_groups: vec![None; I],
            output_eps,
            local_addr,
            input_buffers: [[0; 1500]; I],
            output_buffer: [0; 10000],
            enq_size: 0,
        })
    }
}

impl<const I: usize, const O: usize> Network<I, O> for NativeInterface<I, O> {
    fn can_send(&mut self) -> bool {
        true
    }

    fn recv_directive(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        // Safety: the `recv` implementation promises not to write uninitialised
        // bytes to the `buf`fer, so this casting is safe.
        let buf = unsafe { &mut *(buf as *mut [u8] as *mut [MaybeUninit<u8>]) };
        match self.patch_socket.recv_from(buf) {
            Ok((size, _)) => Ok(size),
            Err(_) => Err(Error::NoData),
        }
    }

    fn send_directive(&mut self, buf: &[u8]) -> Result<(), Error> {
        match self.patch_socket.send_to(buf, &self.patch_ep.into()) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(()),
            Err(_) => Err(Error::Network),
        }
    }

    fn jack_connect(&mut self, jack_id: usize, addr: [u8; 4], time: i64) -> Result<(), Error> {
        if jack_id >= self.input_sockets.len() {
            return Err(Error::InvalidJackId);
        }
        self.jack_disconnect(jack_id, time)?;
        let address = addr.into();
        self.input_sockets[jack_id].join_multicast_v4(&address, &self.local_addr)?;
        self.input_groups[jack_id] = Some(address);
        Ok(())
    }

    fn jack_addr(&mut self, jack_id: usize) -> Result<[u8; 4], Error> {
        if jack_id >= self.output_eps.len() {
            return Err(Error::InvalidJackId);
        }
        Ok(self.output_eps[jack_id].ip().octets())
    }

    fn jack_disconnect(&mut self, jack_id: usize, _time: i64) -> Result<(), Error> {
        if jack_id >= self.input_sockets.len() {
            return Err(Error::InvalidJackId);
        }
        if let Some(old_addr) = self.input_groups[jack_id] {
            self.input_sockets[jack_id].leave_multicast_v4(&old_addr, &self.local_addr)?;
            self.input_groups[jack_id] = None;
        }
        Ok(())
    }

    fn enqueue_packets(&mut self, size: usize) -> Result<[&mut [u8]; O], Error> {
        if size * O > self.output_buffer.len() {
            return Err(Error::StorageFull);
        }
        self.enq_size = size;
        let mut res: [Option<&mut [u8]>; O] = [(); O].map(|_| None);
        for (i, chunk) in self.output_buffer[0..size * O]
            .chunks_exact_mut(size)
            .enumerate()
        {
            res[i] = Some(chunk);
        }
        Ok(res.map(|c| c.unwrap()))
    }

    fn dequeue_packets(&mut self, size: usize) -> ([&[u8]; I], u32) {
        let mut dropped_packets = 0;
        for jack_id in 0..I {
            // Safety: the `recv` implementation promises not to write uninitialised
            // bytes to the `buf`fer, so this casting is safe.
            let buf = unsafe {
                &mut *(&mut self.input_buffers[jack_id][..] as *mut [u8]
                    as *mut [MaybeUninit<u8>])
            };
            match self.input_sockets[jack_id].recv_from(buf) {
                Ok((recv_size, _)) if recv_size == size => {}
                _ => {
                    self.input_buffers[jack_id] = [0; 1500];
                    dropped_packets += 1;
                }
            }
        }
        let mut res: [Option<&[u8]>; I] = [(); I].map(|_| None);
        for (i, buf) in self.input_buffers.iter().enumerate() {
            res[i] = Some(&buf[0..size]);
        }
        (res.map(|c| c.unwrap()), dropped_packets)
    }

    fn poll(&mut self, _time: i64) -> Result<(), Error> {
        if self.enq_size == 0 {
            Ok(())
        } else {
            for i in 0..O {
                match self.patch_socket.send_to(
                    &self.output_buffer[i * self.enq_size..(i + 1) * self.enq_size],
                    &self.output_eps[i].into(),
                ) {
                    Ok(_) => {}
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                    Err(e) => {
                        info!("Jack send error: {:?}", e);
                        return Err(Error::Network);
                    }
                }
            }
            Ok(())
        }
    }
}
