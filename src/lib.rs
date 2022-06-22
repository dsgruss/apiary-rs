#![no_std]

#[macro_use]
extern crate log;

pub mod protocol;
pub mod ui;

const PATCH_PORT: u16 = 19874;
const PATCH_ADDR: [u8; 4] = [239, 0, 0, 0];
const SRC_MAC: [u8; 6] = [0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];

use smoltcp::Error;
use smoltcp::iface::{Interface, InterfaceBuilder, Neighbor, NeighborCache, Route, Routes, SocketHandle, SocketStorage};
use smoltcp::phy::Device;
use smoltcp::socket::{Dhcpv4Event, Dhcpv4Socket, UdpPacketMetadata, UdpSocket, UdpSocketBuffer};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpAddress::Ipv4, IpCidr, IpEndpoint, Ipv4Address, Ipv4Cidr};

use crate::protocol::Directive;

pub struct NetworkInterfaceStorage<'a> {
    ip_addrs: [IpCidr; 1],
    neighbor_storage: [Option<(IpAddress, Neighbor)>; 16],
    routes_storage: [Option<(IpCidr, Route)>; 1],
    ipv4_multicast_storage: [Option<(Ipv4Address, ())>; 1],
    sockets: [SocketStorage<'a>; 2],
    server_rx_metadata_buffer: [UdpPacketMetadata; 4],
    server_rx_payload_buffer: [u8; 2048],
    server_tx_metadata_buffer: [UdpPacketMetadata; 4],
    server_tx_payload_buffer: [u8; 2048],
}

impl NetworkInterfaceStorage<'_> {
    pub fn new() -> Self {
        NetworkInterfaceStorage {
            ip_addrs: [IpCidr::new(Ipv4Address::UNSPECIFIED.into(), 0)],
            neighbor_storage: [None; 16],
            routes_storage: [None; 1],
            ipv4_multicast_storage: [None; 1],
            sockets: Default::default(),
            server_rx_metadata_buffer: [UdpPacketMetadata::EMPTY; 4],
            server_rx_payload_buffer: [0; 2048],
            server_tx_metadata_buffer: [UdpPacketMetadata::EMPTY; 4],
            server_tx_payload_buffer: [0; 2048],
        }
    }
}

pub struct NetworkInterface<'a, DeviceT: for<'d> Device<'d>> {
    iface: Interface<'a, DeviceT>,
    dhcp_handle: SocketHandle,
    dhcp_configured: bool,
    server_handle: SocketHandle,
    broadcast_endpoint: IpEndpoint,
    message_buffer: [u8; 2048],
}

impl<'a, DeviceT> NetworkInterface<'a, DeviceT>
where
    DeviceT: for<'d> Device<'d> 
{
    pub fn new(device: DeviceT, storage: &'a mut NetworkInterfaceStorage<'a>) -> Self {
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
        let server_handle = iface.add_socket(server_socket);
        let broadcast_endpoint =
            IpEndpoint::new(Ipv4(Ipv4Address::from_bytes(&PATCH_ADDR)), PATCH_PORT);

        NetworkInterface {
            iface,
            dhcp_handle,
            dhcp_configured: false,
            server_handle,
            broadcast_endpoint,
            message_buffer: [0; 2048],
        }
    }

    pub fn poll(&mut self, time: i64) -> Result<Option<Directive>, Error> {
        match self.iface.poll(Instant::from_millis(time)) {
            Ok(true) => {
                self.dhcp_poll(time);
                if self.dhcp_configured {
                    let socket = self.iface.get_socket::<UdpSocket>(self.server_handle);
                    if !socket.is_open() {
                        info!("Opening UDP listener socket");
                        if let Err(e) = socket.bind(19874) {
                            info!("UDP listen error: {:?}", e);
                        }
                    }
                }
                Ok(None)
            }
            Ok(false) => {
                Ok(None)
            }
            Err(e) => {
                Err(e)
            }
        }
    }

    pub fn send(&mut self, directive: &Directive) -> Result<(), Error> {
        let socket = self.iface.get_socket::<UdpSocket>(self.server_handle);
        if socket.can_send() && self.dhcp_configured {
            match serde_json_core::to_slice(directive, &mut self.message_buffer) {
                Ok(len) => {
                    socket.send_slice(&self.message_buffer[0..len], self.broadcast_endpoint)?;
                    Ok(())
                }
                Err(_) => {
                    Err(Error::Dropped)
                }
            }
        } else {
            Err(Error::Dropped)
        }
    }

    pub fn can_send(&mut self) -> bool {
        let socket = self.iface.get_socket::<UdpSocket>(self.server_handle);
        socket.can_send() && self.dhcp_configured
    }

    fn dhcp_poll(&mut self, time: i64) {
        let event = self.iface.get_socket::<Dhcpv4Socket>(self.dhcp_handle).poll();
        match event {
            None => {}
            Some(Dhcpv4Event::Configured(config)) => {
                info!("DHCP config acquired!");

                info!("IP address:      {}", config.address);
                self.set_ipv4_addr(config.address);

                if let Some(router) = config.router {
                    info!("Default gateway: {}", router);
                    self.iface.routes_mut().add_default_ipv4_route(router).unwrap();
                } else {
                    info!("Default gateway: None");
                    self.iface.routes_mut().remove_default_ipv4_route();
                }

                for (i, s) in config.dns_servers.iter().enumerate() {
                    if let Some(s) = s {
                        info!("DNS server {}:    {}", i, s);
                    }
                }

                match self.iface.join_multicast_group(
                    Ipv4Address::from_bytes(&PATCH_ADDR),
                    Instant::from_millis(time),
                ) {
                    Ok(sent) => info!("Address added to multicast and sent: {}", sent),
                    Err(e) => info!("Multicast join failed: {}", e),
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

    fn set_ipv4_addr(&mut self, cidr: Ipv4Cidr) {
        self.iface.update_ip_addrs(|addrs| {
            let dest = addrs.iter_mut().next().unwrap();
            *dest = IpCidr::Ipv4(cidr);
        });
    }
}