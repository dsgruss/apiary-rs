/*! smoltcp-based socket interface.

This module provides communication (via the `Network` trait) and basic network management using a `smoltcp`-based network stack, for devices that do not otherwise provide one.
*/

use core::str::FromStr;

use itertools::izip;
use smoltcp::{
    iface::{
        Interface, InterfaceBuilder, Neighbor, NeighborCache, Route, Routes, SocketHandle,
        SocketStorage,
    },
    phy::Device,
    socket::{Dhcpv4Event, Dhcpv4Socket, UdpPacketMetadata, UdpSocket, UdpSocketBuffer, Socket},
    time::Instant,
    wire::{EthernetAddress, IpAddress, IpCidr, IpEndpoint, Ipv4Address, Ipv4Cidr},
};

use crate::{Error, Network, JACK_PORT};

// Until const generics are stabilized, with
// #![feature(const_generics)]
// #![feature(const_evaluatable_checked)]
// Then we need another const which is N = 1 + I + O
pub struct SmoltcpStorage<'a, const I: usize, const O: usize, const N: usize> {
    ip_addrs: [IpCidr; 1],
    neighbor_storage: [Option<(IpAddress, Neighbor)>; 16],
    routes_storage: [Option<(IpCidr, Route)>; 1],
    ipv4_multicast_storage: [Option<(Ipv4Address, ())>; N],
    sockets: [SocketStorage<'a>; 16],
    server_rx_metadata_buffer: [UdpPacketMetadata; 32],
    server_rx_payload_buffer: [u8; 2048],
    server_tx_metadata_buffer: [UdpPacketMetadata; 32],
    server_tx_payload_buffer: [u8; 4096],
    input_jack_rx_metadata_buffers: [[UdpPacketMetadata; 16]; I],
    input_jack_rx_payload_buffers: [[u8; 4096]; I],
    input_jack_tx_metadata_buffers: [[UdpPacketMetadata; 0]; I],
    input_jack_tx_payload_buffers: [[u8; 0]; I],
    output_jack_rx_metadata_buffers: [[UdpPacketMetadata; 0]; O],
    output_jack_rx_payload_buffers: [[u8; 0]; O],
    output_jack_tx_metadata_buffers: [[UdpPacketMetadata; 16]; O],
    output_jack_tx_payload_buffers: [[u8; 4096]; O],
}

impl<const I: usize, const O: usize, const N: usize> Default for SmoltcpStorage<'_, I, O, N> {
    fn default() -> Self {
        SmoltcpStorage {
            ip_addrs: [IpCidr::new(Ipv4Address::UNSPECIFIED.into(), 0)],
            neighbor_storage: [None; 16],
            routes_storage: [None; 1],
            ipv4_multicast_storage: [None; N],
            sockets: [0; 16].map(|_| Default::default()), // This the best way to do this?
            server_rx_metadata_buffer: [UdpPacketMetadata::EMPTY; 32],
            server_rx_payload_buffer: [0; 2048],
            server_tx_metadata_buffer: [UdpPacketMetadata::EMPTY; 32],
            server_tx_payload_buffer: [0; 4096],
            input_jack_rx_metadata_buffers: [[UdpPacketMetadata::EMPTY; 16]; I],
            input_jack_rx_payload_buffers: [[0; 4096]; I],
            input_jack_tx_metadata_buffers: [[UdpPacketMetadata::EMPTY; 0]; I],
            input_jack_tx_payload_buffers: [[0; 0]; I],
            output_jack_rx_metadata_buffers: [[UdpPacketMetadata::EMPTY; 0]; O],
            output_jack_rx_payload_buffers: [[0; 0]; O],
            output_jack_tx_metadata_buffers: [[UdpPacketMetadata::EMPTY; 16]; O],
            output_jack_tx_payload_buffers: [[0; 4096]; O],
        }
    }
}

pub struct SmoltcpInterface<
    'a,
    DeviceT: for<'d> Device<'d>,
    const I: usize,
    const O: usize,
    const N: usize,
> {
    iface: Interface<'a, DeviceT>,
    dhcp_handle: SocketHandle,
    dhcp_configured: bool,
    server_handle: SocketHandle,
    broadcast_endpoint: IpEndpoint,
    input_jack_handles: [SocketHandle; I],
    input_jack_endpoints: [Option<IpEndpoint>; I],
    output_jack_handles: [SocketHandle; O],
    output_jack_endpoints: [IpEndpoint; O],
}

impl<'a, DeviceT, const I: usize, const O: usize, const N: usize>
    SmoltcpInterface<'a, DeviceT, I, O, N>
where
    DeviceT: for<'d> Device<'d>,
{
    pub fn new(
        device: DeviceT,
        src_mac: [u8; 6],
        storage: &'a mut SmoltcpStorage<'a, I, O, N>,
    ) -> Self {
        let neighbor_cache = NeighborCache::new(&mut storage.neighbor_storage[..]);
        let routes = Routes::new(&mut storage.routes_storage[..]);
        let ethernet_addr = EthernetAddress(src_mac);

        let mut iface = InterfaceBuilder::new(device, &mut storage.sockets[..])
            .hardware_addr(ethernet_addr.into())
            .ip_addrs(&mut storage.ip_addrs[..])
            .routes(routes)
            .neighbor_cache(neighbor_cache)
            .ipv4_multicast_groups(&mut storage.ipv4_multicast_storage[..])
            .finalize();

        let dhcp_socket = Dhcpv4Socket::new();
        let dhcp_handle = iface.add_socket(dhcp_socket);

        let server_socket = UdpSocket::new(
            UdpSocketBuffer::new(
                &mut storage.server_rx_metadata_buffer[..],
                &mut storage.server_rx_payload_buffer[..],
            ),
            UdpSocketBuffer::new(
                &mut storage.server_tx_metadata_buffer[..],
                &mut storage.server_tx_payload_buffer[..],
            ),
        );
        let server_handle = iface.add_socket(server_socket);

        let mut input_jack_handles: [SocketHandle; I] = [Default::default(); I];

        let mut i = 0;
        for (rx_meta, rx_payload, tx_meta, tx_payload) in izip!(
            storage.input_jack_rx_metadata_buffers.iter_mut(),
            storage.input_jack_rx_payload_buffers.iter_mut(),
            storage.input_jack_tx_metadata_buffers.iter_mut(),
            storage.input_jack_tx_payload_buffers.iter_mut(),
        ) {
            let input_jack_socket = UdpSocket::new(
                UdpSocketBuffer::new(&mut rx_meta[..], &mut rx_payload[..]),
                UdpSocketBuffer::new(&mut tx_meta[..], &mut tx_payload[..]),
            );
            input_jack_handles[i] = iface.add_socket(input_jack_socket);
            i += 1;
        }

        let mut output_jack_handles: [SocketHandle; O] = [Default::default(); O];

        let mut i = 0;
        for (rx_meta, rx_payload, tx_meta, tx_payload) in izip!(
            storage.output_jack_rx_metadata_buffers.iter_mut(),
            storage.output_jack_rx_payload_buffers.iter_mut(),
            storage.output_jack_tx_metadata_buffers.iter_mut(),
            storage.output_jack_tx_payload_buffers.iter_mut(),
        ) {
            let output_jack_socket = UdpSocket::new(
                UdpSocketBuffer::new(&mut rx_meta[..], &mut rx_payload[..]),
                UdpSocketBuffer::new(&mut tx_meta[..], &mut tx_payload[..]),
            );
            output_jack_handles[i] = iface.add_socket(output_jack_socket);
            i += 1;
        }
        let broadcast_endpoint = IpEndpoint::from_str(crate::PATCH_EP).unwrap();

        SmoltcpInterface {
            iface,
            dhcp_handle,
            dhcp_configured: false,
            server_handle,
            broadcast_endpoint,
            input_jack_handles,
            output_jack_handles,
            input_jack_endpoints: [None; I],
            output_jack_endpoints: [IpEndpoint::UNSPECIFIED; O],
        }
    }

    fn set_ipv4_addr(&mut self, cidr: Ipv4Cidr) {
        self.iface.update_ip_addrs(|addrs| {
            let dest = addrs.iter_mut().next().unwrap();
            *dest = IpCidr::Ipv4(cidr);
        });
    }

    fn dhcp_poll(&mut self, time: i64) {
        let event = self
            .iface
            .get_socket::<Dhcpv4Socket>(self.dhcp_handle)
            .poll();
        match event {
            None => {}
            Some(Dhcpv4Event::Configured(config)) => {
                info!("DHCP config acquired!");

                info!("IP address:      {}", config.address);
                self.set_ipv4_addr(config.address.clone());
                let addr = config.address.address();
                let addr_bytes = addr.as_bytes();
                for i in 0..O {
                    let jack_addr = Ipv4Address::new(239, addr_bytes[2], addr_bytes[3], i as u8);
                    self.output_jack_endpoints[i] =
                        IpEndpoint::new(IpAddress::Ipv4(jack_addr), JACK_PORT);
                }

                if let Some(router) = config.router {
                    info!("Default gateway: {}", router);
                    self.iface
                        .routes_mut()
                        .add_default_ipv4_route(router)
                        .unwrap();
                } else {
                    info!("Default gateway: None");
                    self.iface.routes_mut().remove_default_ipv4_route();
                }

                for (i, s) in config.dns_servers.iter().enumerate() {
                    if let Some(s) = s {
                        info!("DNS server {}:    {}", i, s);
                    }
                }

                match self
                    .iface
                    .join_multicast_group(self.broadcast_endpoint.addr, Instant::from_millis(time))
                {
                    Ok(sent) => info!(
                        "Address added to patch management and sent: {:?} {}",
                        self.broadcast_endpoint.addr, sent
                    ),
                    Err(e) => info!("Multicast join failed: {}", e),
                }
                for ep in self.output_jack_endpoints {
                    match self
                        .iface
                        .join_multicast_group(ep.addr, Instant::from_millis(time))
                    {
                        Ok(sent) => info!(
                            "Address added to multicast and sent: {:?} {}",
                            ep.addr, sent
                        ),
                        Err(e) => info!("Multicast join failed: {}", e),
                    }
                }
                self.dhcp_configured = true;
            }
            Some(Dhcpv4Event::Deconfigured) => {
                info!("DHCP lost config!");
                self.set_ipv4_addr(Ipv4Cidr::new(Ipv4Address::UNSPECIFIED, 0));
                self.iface.routes_mut().remove_default_ipv4_route();
                self.dhcp_configured = false;
            }
        }
    }
}

impl<'a, DeviceT, const I: usize, const O: usize, const N: usize> Network<I, O>
    for SmoltcpInterface<'a, DeviceT, I, O, N>
where
    DeviceT: for<'d> Device<'d>,
{
    fn poll(&mut self, time: i64) -> Result<bool, Error> {
        match self.iface.poll(Instant::from_millis(time)) {
            Ok(_) => {
                self.dhcp_poll(time);
                if self.dhcp_configured {
                    let socket = self.iface.get_socket::<UdpSocket>(self.server_handle);
                    if !socket.is_open() {
                        info!("Opening UDP listener socket");
                        if let Err(_) = socket.bind(self.broadcast_endpoint.port) {
                            return Err(Error::Network);
                        }
                    }
                    let mut port = 30000;
                    for h in self.output_jack_handles {
                        let socket = self.iface.get_socket::<UdpSocket>(h);
                        if !socket.is_open() {
                            if let Err(_) = socket.bind(port) {
                                return Err(Error::Network);
                            }
                            port += 1;
                        }
                    }
                }
                Ok(true)
            }
            Err(_) => Err(Error::Network),
        }
    }

    fn can_send(&mut self) -> bool {
        let socket = self.iface.get_socket::<UdpSocket>(self.server_handle);
        // Perhaps check all sockets?
        socket.can_send() && self.dhcp_configured
    }

    fn recv_directive(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        let socket = self.iface.get_socket::<UdpSocket>(self.server_handle);
        if socket.can_recv() && self.dhcp_configured {
            match socket.recv_slice(buf) {
                Ok((size, _)) => Ok(size),
                Err(_) => Err(Error::Network),
            }
        } else {
            Err(Error::NoData)
        }
    }

    fn send_directive(&mut self, buf: &[u8]) -> Result<(), Error> {
        let socket = self.iface.get_socket::<UdpSocket>(self.server_handle);
        if socket.can_send() && self.dhcp_configured {
            match socket.send_slice(buf, self.broadcast_endpoint) {
                Err(_) => Err(Error::Network),
                Ok(_) => Ok(()),
            }
        } else {
            Err(Error::Network)
        }
    }

    fn jack_connect(&mut self, jack_id: usize, addr: [u8; 4], time: i64) -> Result<(), Error> {
        let address = Ipv4Address::from_bytes(&addr);
        let t = Instant::from_millis(time);
        let ep = IpEndpoint::new(IpAddress::Ipv4(address), JACK_PORT);
        self.jack_disconnect(jack_id, time)?;
        info!(
            "Input jack {}: Joining group {:?} and opening socket",
            jack_id, ep
        );
        if let Err(_) = self.iface.join_multicast_group(ep.addr, t) {
            return Err(Error::Network);
        }
        self.input_jack_endpoints[jack_id] = Some(ep);
        let jack_socket = self
            .iface
            .get_socket::<UdpSocket>(self.input_jack_handles[jack_id]);
        jack_socket.bind(ep).or(Err(Error::Network))
    }

    fn jack_recv(&mut self, jack_id: usize, buf: &mut [u8]) -> Result<usize, Error> {
        let jack_socket = self
            .iface
            .get_socket::<UdpSocket>(self.input_jack_handles[jack_id]);
        if jack_socket.can_recv() && self.dhcp_configured {
            match jack_socket.recv_slice(buf) {
                Ok((size, _)) => Ok(size),
                Err(_) => Err(Error::Network),
            }
        } else {
            Err(Error::NoData)
        }
    }

    fn jack_send(&mut self, jack_id: usize, buf: &[u8]) -> Result<(), Error> {
        let socket = self.iface.get_socket::<UdpSocket>(self.output_jack_handles[jack_id]);
        if socket.can_send()
            && self.dhcp_configured
            && self.output_jack_endpoints[jack_id].is_specified()
        {
            match socket.send_slice(buf, self.output_jack_endpoints[jack_id]) {
                Err(e) => {
                    info!("Send slice error: {:?}, {:?} {:?}", e, jack_id, self.output_jack_endpoints[jack_id]);
                    Err(Error::Network)
                }
                Ok(_) => Ok(()),
            }
        } else {
            info!("Socket not ready");
            Err(Error::Network)
        }
    }

    fn enqueue_packets(&mut self, size: usize) -> [&mut [u8]; O] {
        let mut res: [Option<&mut [u8]>; O] = [(); O].map(|_| None);
        for (h, s) in self.iface.sockets_mut() {
            match s {
                Socket::Udp(s) => {
                    for i in 0..O {
                        if self.output_jack_handles[i] == h {
                            res[i] = Some(s.send(size, self.output_jack_endpoints[i]).unwrap());
                            break;
                        }
                    }
                }
                _ => {}
            };
        }
        res.map(|s| s.unwrap())
        // let res = for (i, out) in self.output_jack_handles.iter().enumerate() {
        //     let socket = self.iface.get_socket::<UdpSocket>(*out);
        //     socket.send(size, self.output_jack_endpoints[i]).unwrap()
        // }
    }

    fn jack_addr(&mut self, jack_id: usize) -> Result<[u8; 4], Error> {
        self.output_jack_endpoints[jack_id]
            .addr
            .as_bytes()
            .try_into()
            .or(Err(Error::InvalidJackId))
    }

    fn jack_disconnect(&mut self, jack_id: usize, time: i64) -> Result<(), Error> {
        let t = Instant::from_millis(time);
        if let Some(old_ep) = self.input_jack_endpoints[jack_id] {
            if let Err(_) = self.iface.leave_multicast_group(old_ep.addr, t) {
                return Err(Error::Network);
            }
            info!("Input jack {}: Leaving group", jack_id);
        }
        let jack_socket = self
            .iface
            .get_socket::<UdpSocket>(self.input_jack_handles[jack_id]);
        if jack_socket.is_open() {
            jack_socket.close();
        }
        Ok(())
    }
}
