/*! smoltcp-based socket interface.

This module provides communication (via the `Network` trait) and basic network management using a `smoltcp`-based network stack, for devices that do not otherwise provide one.
*/

use core::str::FromStr;

use smoltcp::{
    iface::{
        Interface, InterfaceBuilder, Neighbor, NeighborCache, Route, Routes, SocketHandle,
        SocketStorage,
    },
    phy::Device,
    socket::{Dhcpv4Event, Dhcpv4Socket, UdpPacketMetadata, UdpSocket, UdpSocketBuffer},
    time::Instant,
    wire::{EthernetAddress, IpAddress, IpCidr, IpEndpoint, Ipv4Address, Ipv4Cidr},
};

use crate::{Error, Network, JACK_PORT};

const OUTPUT_JACK_EP: &str = "239.1.2.3:19991";
const SRC_MAC: [u8; 6] = [0x00, 0x00, 0xca, 0x55, 0xe7, 0x7e];

pub struct SmoltcpStorage<'a> {
    ip_addrs: [IpCidr; 1],
    neighbor_storage: [Option<(IpAddress, Neighbor)>; 16],
    routes_storage: [Option<(IpCidr, Route)>; 1],
    ipv4_multicast_storage: [Option<(Ipv4Address, ())>; 3],
    sockets: [SocketStorage<'a>; 3],
    server_rx_metadata_buffer: [UdpPacketMetadata; 4],
    server_rx_payload_buffer: [u8; 2048],
    server_tx_metadata_buffer: [UdpPacketMetadata; 4],
    server_tx_payload_buffer: [u8; 2048],
    jack_rx_metadata_buffers: [[UdpPacketMetadata; 4]; 1],
    jack_rx_payload_buffers: [[u8; 2048]; 1],
    jack_tx_metadata_buffers: [[UdpPacketMetadata; 4]; 1],
    jack_tx_payload_buffers: [[u8; 2048]; 1],
}

impl Default for SmoltcpStorage<'_> {
    fn default() -> Self {
        SmoltcpStorage {
            ip_addrs: [IpCidr::new(Ipv4Address::UNSPECIFIED.into(), 0)],
            neighbor_storage: [None; 16],
            routes_storage: [None; 1],
            ipv4_multicast_storage: [None; 3],
            sockets: Default::default(),
            server_rx_metadata_buffer: [UdpPacketMetadata::EMPTY; 4],
            server_rx_payload_buffer: [0; 2048],
            server_tx_metadata_buffer: [UdpPacketMetadata::EMPTY; 4],
            server_tx_payload_buffer: [0; 2048],
            jack_rx_metadata_buffers: [[UdpPacketMetadata::EMPTY; 4]; 1],
            jack_rx_payload_buffers: [[0; 2048]; 1],
            jack_tx_metadata_buffers: [[UdpPacketMetadata::EMPTY; 4]; 1],
            jack_tx_payload_buffers: [[0; 2048]; 1],
        }
    }
}

pub struct SmoltcpInterface<'a, DeviceT: for<'d> Device<'d>> {
    iface: Interface<'a, DeviceT>,
    dhcp_handle: SocketHandle,
    dhcp_configured: bool,
    server_handle: SocketHandle,
    broadcast_endpoint: IpEndpoint,
    input_jack_handle: SocketHandle,
    output_jack_endpoint: IpEndpoint,
    input_jack_endpoint: Option<IpEndpoint>,
}

impl<'a, DeviceT> SmoltcpInterface<'a, DeviceT>
where
    DeviceT: for<'d> Device<'d>,
{
    pub fn new(device: DeviceT, storage: &'a mut SmoltcpStorage<'a>) -> Self {
        let neighbor_cache = NeighborCache::new(&mut storage.neighbor_storage[..]);
        let routes = Routes::new(&mut storage.routes_storage[..]);
        let ethernet_addr = EthernetAddress(SRC_MAC);

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
        let input_jack_socket = UdpSocket::new(
            UdpSocketBuffer::new(
                &mut storage.jack_rx_metadata_buffers[0][..],
                &mut storage.jack_rx_payload_buffers[0][..],
            ),
            UdpSocketBuffer::new(
                &mut storage.jack_tx_metadata_buffers[0][..],
                &mut storage.jack_tx_payload_buffers[0][..],
            ),
        );
        let server_handle = iface.add_socket(server_socket);
        let broadcast_endpoint = IpEndpoint::from_str(crate::PATCH_EP).unwrap();
        let input_jack_handle = iface.add_socket(input_jack_socket);
        let output_jack_endpoint = IpEndpoint::from_str(OUTPUT_JACK_EP).unwrap();

        SmoltcpInterface {
            iface,
            dhcp_handle,
            dhcp_configured: false,
            server_handle,
            broadcast_endpoint,
            input_jack_handle,
            output_jack_endpoint,
            input_jack_endpoint: None,
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
                self.set_ipv4_addr(config.address);

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

                for ep in [self.broadcast_endpoint, self.output_jack_endpoint] {
                    match self
                        .iface
                        .join_multicast_group(ep.addr, Instant::from_millis(time))
                    {
                        Ok(sent) => info!("Address added to multicast and sent: {}", sent),
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

impl<'a, DeviceT> Network for SmoltcpInterface<'a, DeviceT>
where
    DeviceT: for<'d> Device<'d>,
{
    fn poll(&mut self, time: i64) -> Result<bool, Error> {
        match self.iface.poll(Instant::from_millis(time)) {
            Ok(true) => {
                self.dhcp_poll(time);
                if self.dhcp_configured {
                    let socket = self.iface.get_socket::<UdpSocket>(self.server_handle);
                    if !socket.is_open() {
                        info!("Opening UDP listener socket");
                        if let Err(_) = socket.bind(self.broadcast_endpoint.port) {
                            return Err(Error::Network);
                        }
                    }
                }
                Ok(true)
            }
            Ok(false) => Ok(true),
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

    fn jack_connect(&mut self, _jack_id: usize, addr: [u8; 4], time: i64) -> Result<(), Error> {
        let address = Ipv4Address::from_bytes(&addr);
        let t = Instant::from_millis(time);
        let ep = IpEndpoint::new(IpAddress::Ipv4(address), JACK_PORT);
        if let Some(old_ep) = self.input_jack_endpoint {
            if let Err(_) = self.iface.leave_multicast_group(old_ep.addr, t) {
                return Err(Error::Network);
            }
            info!("Input jack 0: Leaving group");
        }
        info!("Input jack 0: Joining group and opening socket");
        if let Err(_) = self.iface.join_multicast_group(ep.addr, t) {
            return Err(Error::Network);
        }
        self.input_jack_endpoint = Some(ep);
        let jack_socket = self.iface.get_socket::<UdpSocket>(self.input_jack_handle);
        if jack_socket.is_open() {
            jack_socket.close();
        }
        jack_socket.bind(ep).or(Err(Error::Network))
    }

    fn jack_recv(&mut self, _jack_id: usize, buf: &mut [u8]) -> Result<usize, Error> {
        let jack_socket = self.iface.get_socket::<UdpSocket>(self.input_jack_handle);
        if jack_socket.can_recv() && self.dhcp_configured {
            match jack_socket.recv_slice(buf) {
                Ok((size, _)) => Ok(size),
                Err(_) => Err(Error::Network),
            }
        } else {
            Err(Error::NoData)
        }
    }

    fn jack_send(&mut self, _jack_id: usize, buf: &[u8]) -> Result<(), Error> {
        let socket = self.iface.get_socket::<UdpSocket>(self.server_handle);
        if socket.can_send() && self.dhcp_configured {
            match socket.send_slice(buf, self.output_jack_endpoint) {
                Err(_) => Err(Error::Network),
                Ok(_) => Ok(()),
            }
        } else {
            Err(Error::Network)
        }
    }

    fn jack_addr(&mut self, _jack_id: usize) -> Result<[u8; 4], Error> {
        self.output_jack_endpoint
            .addr
            .as_bytes()
            .try_into()
            .or(Err(Error::InvalidJackId))
    }
}
