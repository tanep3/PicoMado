//! BT-AUDIO-20261107: connection-scoped, optional Welcome link hint.
//! Pure admission uses accepted endpoints and OS facts, never learned neighbors.
use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

pub const CAPABILITY: &str = "server-link-hint-v1";
const LOOKUP_BUDGET: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerLinkHint {
    pub version: u8,
    pub server_ipv4: Ipv4Addr,
    pub client_ipv4: Ipv4Addr,
    pub prefix_length: u8,
    pub mac: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcceptedEndpoints {
    pub local: SocketAddr,
    pub peer: SocketAddr,
}
impl AcceptedEndpoints {
    fn ipv4(self) -> Option<(Ipv4Addr, Ipv4Addr)> {
        let (SocketAddr::V4(local), SocketAddr::V4(peer)) = (self.local, self.peer) else {
            return None;
        };
        let (local, peer) = (*local.ip(), *peer.ip());
        (ordinary_ipv4(local) && ordinary_ipv4(peer) && local != peer).then_some((local, peer))
    }
}
fn ordinary_ipv4(ip: Ipv4Addr) -> bool {
    !ip.is_unspecified()
        && !ip.is_loopback()
        && !ip.is_multicast()
        && !ip.is_broadcast()
        && ip.octets()[0] != 0
        && ip.octets()[0] < 224
}
fn valid_mac(mac: &[u8; 6]) -> bool {
    mac[0] & 1 == 0 && *mac != [0; 6]
}
fn prefix(mask: Ipv4Addr) -> Option<u8> {
    let mask = u32::from(mask);
    let bits = mask.leading_ones();
    (bits >= 1 && bits <= 30 && mask == u32::MAX << (32 - bits)).then_some(bits as u8)
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Interface {
    index: u32,
    address: Ipv4Addr,
    mask: Ipv4Addr,
    mac: [u8; 6],
    eligible: bool,
}
#[derive(Clone, Copy, Debug)]
struct DirectRoute {
    destination: Ipv4Addr,
    source: Ipv4Addr,
    index: u32,
}
fn admit(
    endpoints: AcceptedEndpoints,
    interfaces: &[Interface],
    route: DirectRoute,
) -> Option<ServerLinkHint> {
    let (local, peer) = endpoints.ipv4()?;
    let mut owners = interfaces.iter().filter(|i| i.address == local);
    let owner = owners.next()?;
    // Duplicate identical records are one NIC; conflicting ownership or facts
    // (including an ineligible second owner) make this address ambiguous.
    if owners.any(|other| other != owner)
        || !owner.eligible
        || owner.index == 0
        || !valid_mac(&owner.mac)
    {
        return None;
    }
    let bits = prefix(owner.mask)?;
    let mask = u32::from(owner.mask);
    let host = |ip: Ipv4Addr| {
        let suffix = u32::from(ip) & !mask;
        suffix != 0 && suffix != !mask
    };
    if !host(local)
        || !host(peer)
        || u32::from(local) & mask != u32::from(peer) & mask
        || route.destination != peer
        || route.source != local
        || route.index != owner.index
    {
        return None;
    }
    Some(ServerLinkHint {
        version: 1,
        server_ipv4: local,
        client_ipv4: peer,
        prefix_length: bits,
        mac: owner
            .mac
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":"),
    })
}

pub(crate) struct FreshHint {
    hint: ServerLinkHint,
    deadline: Instant,
}
impl FreshHint {
    #[cfg(test)]
    pub(crate) fn for_test(hint: ServerLinkHint, deadline: Instant) -> Self {
        Self { hint, deadline }
    }
    pub(crate) fn into_current(self) -> Option<ServerLinkHint> {
        (Instant::now() < self.deadline).then_some(self.hint)
    }
}

// No App lock is held across this adapter. Saturation, blocking-pool queue time,
// lookup failures and timeout all omit the optional hint, not the Welcome.
pub(crate) async fn lookup(endpoints: AcceptedEndpoints) -> Option<FreshHint> {
    static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
    lookup_bounded(endpoints, LOOKUP_BUDGET, &SLOTS, lookup_sync).await
}
async fn lookup_bounded<F>(
    endpoints: AcceptedEndpoints,
    budget: Duration,
    slots: &'static tokio::sync::Semaphore,
    query: F,
) -> Option<FreshHint>
where
    F: FnOnce(AcceptedEndpoints, Instant) -> Option<ServerLinkHint> + Send + 'static,
{
    endpoints.ipv4()?;
    let permit = slots.try_acquire().ok()?;
    let deadline = Instant::now() + budget;
    let task = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if Instant::now() >= deadline {
            return None;
        }
        query(endpoints, deadline)
    });
    let hint = tokio::time::timeout(budget, task).await.ok()?.ok()??;
    (Instant::now() < deadline).then_some(FreshHint { hint, deadline })
}
#[cfg(not(target_os = "linux"))]
fn lookup_sync(_: AcceptedEndpoints, _: Instant) -> Option<ServerLinkHint> {
    None
}
#[cfg(target_os = "linux")]
fn lookup_sync(endpoints: AcceptedEndpoints, deadline: Instant) -> Option<ServerLinkHint> {
    let (local, peer) = endpoints.ipv4()?;
    let before = linux::interfaces().ok()?;
    let owners: Vec<_> = before.iter().filter(|i| i.address == local).collect();
    if owners.is_empty() || owners.iter().any(|o| *o != owners[0]) || !owners[0].eligible {
        return None;
    }
    let index = owners[0].index;
    let route = linux::direct_route(local, peer, index, deadline).ok()?;
    let hint = admit(endpoints, &before, route)?;
    // Re-read ownership, mask and current AF_PACKET MAC after the route query.
    let after = linux::interfaces().ok()?;
    if admit(endpoints, &after, route)? != hint || Instant::now() >= deadline {
        return None;
    }
    Some(hint)
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    fn invalid() -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported or ambiguous link/route",
        )
    }
    pub(super) fn interfaces() -> io::Result<Vec<Interface>> {
        let mut head = std::ptr::null_mut();
        // libc owns an immutable linked snapshot until freeifaddrs.
        if unsafe { libc::getifaddrs(&mut head) } != 0 {
            return Err(io::Error::last_os_error());
        }
        struct Snapshot(*mut libc::ifaddrs);
        impl Drop for Snapshot {
            fn drop(&mut self) {
                unsafe {
                    libc::freeifaddrs(self.0);
                }
            }
        }
        let _snapshot = Snapshot(head);
        let mut addresses = Vec::new();
        let mut links = Vec::new();
        let mut cursor = head;
        let mut count = 0;
        while !cursor.is_null() {
            count += 1;
            if count > 4096 {
                return Err(invalid());
            }
            let entry = unsafe { &*cursor };
            cursor = entry.ifa_next;
            if entry.ifa_addr.is_null() || entry.ifa_name.is_null() {
                continue;
            }
            let name = unsafe { std::ffi::CStr::from_ptr(entry.ifa_name) }
                .to_bytes()
                .to_vec();
            let flags = entry.ifa_flags as i32;
            match unsafe { (*entry.ifa_addr).sa_family } as i32 {
                libc::AF_INET => {
                    let address = unsafe { &*(entry.ifa_addr as *const libc::sockaddr_in) };
                    let mask = if !entry.ifa_netmask.is_null()
                        && unsafe { (*entry.ifa_netmask).sa_family } as i32 == libc::AF_INET
                    {
                        unsafe {
                            (*(entry.ifa_netmask as *const libc::sockaddr_in))
                                .sin_addr
                                .s_addr
                        }
                    } else {
                        0
                    };
                    addresses.push((
                        name,
                        Ipv4Addr::from(address.sin_addr.s_addr.to_ne_bytes()),
                        Ipv4Addr::from(mask.to_ne_bytes()),
                        flags,
                    ));
                }
                libc::AF_PACKET => {
                    let link = unsafe { &*(entry.ifa_addr as *const libc::sockaddr_ll) };
                    if link.sll_hatype == libc::ARPHRD_ETHER
                        && link.sll_halen == 6
                        && link.sll_ifindex > 0
                    {
                        let mut mac = [0; 6];
                        mac.copy_from_slice(&link.sll_addr[..6]);
                        links.push((name, link.sll_ifindex as u32, mac));
                    }
                }
                _ => {}
            }
        }
        Ok(addresses
            .into_iter()
            .map(|(name, address, mask, flags)| {
                let matching: Vec<_> = links.iter().filter(|(n, _, _)| *n == name).collect();
                let (index, mac) = if matching.len() == 1 {
                    (matching[0].1, matching[0].2)
                } else {
                    (0, [0; 6])
                };
                Interface {
                    index,
                    address,
                    mask,
                    mac,
                    eligible: matching.len() == 1
                        && flags & libc::IFF_UP != 0
                        && flags & libc::IFF_RUNNING != 0
                        && flags & (libc::IFF_LOOPBACK | libc::IFF_POINTOPOINT | libc::IFF_NOARP)
                            == 0,
                }
            })
            .collect())
    }
    fn u16_at(bytes: &[u8], at: usize) -> io::Result<u16> {
        Ok(u16::from_ne_bytes(
            bytes
                .get(at..at + 2)
                .ok_or_else(invalid)?
                .try_into()
                .unwrap(),
        ))
    }
    fn u32_at(bytes: &[u8], at: usize) -> io::Result<u32> {
        Ok(u32::from_ne_bytes(
            bytes
                .get(at..at + 4)
                .ok_or_else(invalid)?
                .try_into()
                .unwrap(),
        ))
    }
    fn attrs(mut data: &[u8]) -> io::Result<Vec<(u16, &[u8])>> {
        let mut result = Vec::new();
        while !data.is_empty() {
            let len = u16_at(data, 0)? as usize;
            let kind = u16_at(data, 2)?;
            if len < 4 || len > data.len() || result.len() >= 128 {
                return Err(invalid());
            }
            // Attribute type flag bits have their own semantics; reject them
            // for interpreted fields instead of accidentally accepting a claim.
            result.push((kind, &data[4..len]));
            let padded = (len + 3) & !3;
            data = data.get(padded..).ok_or_else(invalid)?;
        }
        Ok(result)
    }
    fn add_attr(data: &mut Vec<u8>, kind: u16, bytes: &[u8]) {
        data.extend_from_slice(&((4 + bytes.len()) as u16).to_ne_bytes());
        data.extend_from_slice(&kind.to_ne_bytes());
        data.extend_from_slice(bytes);
        while data.len() % 4 != 0 {
            data.push(0);
        }
    }
    // Single-request private socket, kernel sender, exact sequence, one result,
    // no multicast, multipart, truncated datagrams, indefinite receive or dump.
    fn query(kind: u16, expected: u16, payload: &[u8], deadline: Instant) -> io::Result<Vec<u8>> {
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let raw = unsafe {
            libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_RAW | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                libc::NETLINK_ROUTE,
            )
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let mut kernel: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
        kernel.nl_family = libc::AF_NETLINK as u16;
        let mut request = Vec::new();
        request.extend_from_slice(&((16 + payload.len()) as u32).to_ne_bytes());
        request.extend_from_slice(&kind.to_ne_bytes());
        request.extend_from_slice(&(libc::NLM_F_REQUEST as u16).to_ne_bytes());
        request.extend_from_slice(&1u32.to_ne_bytes());
        request.extend_from_slice(&0u32.to_ne_bytes());
        request.extend_from_slice(payload);
        let sent = unsafe {
            libc::sendto(
                fd.as_raw_fd(),
                request.as_ptr().cast(),
                request.len(),
                0,
                (&kernel as *const libc::sockaddr_nl).cast(),
                std::mem::size_of_val(&kernel) as libc::socklen_t,
            )
        };
        if sent != request.len() as isize {
            return Err(io::Error::last_os_error());
        }
        loop {
            let left = deadline
                .checked_duration_since(Instant::now())
                .ok_or(io::ErrorKind::TimedOut)?;
            let mut poll = libc::pollfd {
                fd: fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let result =
                unsafe { libc::poll(&mut poll, 1, left.as_millis().max(1).min(100) as i32) };
            if result < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if result < 0 {
                return Err(io::Error::last_os_error());
            }
            if result == 0 || Instant::now() >= deadline {
                return Err(io::ErrorKind::TimedOut.into());
            }
            if poll.revents != libc::POLLIN {
                return Err(invalid());
            }
            break;
        }
        let mut buffer = [0u8; 8192];
        let mut sender: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
        let mut sender_len = std::mem::size_of_val(&sender) as libc::socklen_t;
        let received = unsafe {
            libc::recvfrom(
                fd.as_raw_fd(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                libc::MSG_TRUNC,
                (&mut sender as *mut libc::sockaddr_nl).cast(),
                &mut sender_len,
            )
        };
        if received < 0 {
            return Err(io::Error::last_os_error());
        }
        if received as usize > buffer.len()
            || sender_len != std::mem::size_of_val(&sender) as u32
            || sender.nl_family != libc::AF_NETLINK as u16
            || sender.nl_pid != 0
            || sender.nl_groups != 0
        {
            return Err(invalid());
        }
        parse_response(&buffer[..received as usize], expected)
    }
    fn parse_response(data: &[u8], expected: u16) -> io::Result<Vec<u8>> {
        let len = u32_at(data, 0)? as usize;
        if len < 16
            || (len + 3) & !3 != data.len()
            || u16_at(data, 4)? != expected
            || u16_at(data, 6)? & (libc::NLM_F_MULTI | libc::NLM_F_DUMP_INTR) as u16 != 0
            || u32_at(data, 8)? != 1
        {
            return Err(invalid());
        }
        Ok(data[16..len].to_vec())
    }
    fn check_link(data: &[u8], index: u32) -> io::Result<()> {
        if data.len() < 16 || u16_at(data, 2)? != libc::ARPHRD_ETHER || u32_at(data, 4)? != index {
            return Err(invalid());
        }
        let flags = u32_at(data, 8)? as i32;
        if flags & libc::IFF_UP == 0
            || flags & libc::IFF_RUNNING == 0
            || flags & (libc::IFF_LOOPBACK | libc::IFF_POINTOPOINT | libc::IFF_NOARP) != 0
        {
            return Err(invalid());
        }
        for (kind, value) in attrs(&data[16..])? {
            match kind & 0x3fff {
                // IFLA_LINKINFO excludes VLAN, bond, bridge, VRF and tunnels.
                18 => return Err(invalid()),
                10 if value != 0u32.to_ne_bytes() => return Err(invalid()), // IFLA_MASTER
                5 if value != index.to_ne_bytes() => return Err(invalid()), // IFLA_LINK
                37 => return Err(invalid()),                                // IFLA_LINK_NETNSID
                _ => {}
            }
        }
        Ok(())
    }
    fn parse_route(
        data: &[u8],
        source: Ipv4Addr,
        destination: Ipv4Addr,
        index: u32,
    ) -> io::Result<DirectRoute> {
        // rtmsg: family, dst_len, src_len, tos, table, protocol, scope, type, flags.
        if data.len() < 12
            || data[0] != libc::AF_INET as u8
            || data[1] != 32
            || data[2] != 32
            || data[3] != 0
            || data[7] != 1
            || u32_at(data, 8)? & !0x200 != 0
        {
            return Err(invalid());
        } // RTN_UNICAST, RTM_F_CLONED only
        let (mut dst, mut src, mut oif) = (None, None, None);
        for (kind, value) in attrs(&data[12..])? {
            match kind & 0x3fff {
                1 | 2 | 4 => {
                    if kind & 0xc000 != 0 || value.len() != 4 {
                        return Err(invalid());
                    }
                    let slot = match kind {
                        1 => &mut dst,
                        2 => &mut src,
                        _ => &mut oif,
                    };
                    if slot.replace(<[u8; 4]>::try_from(value).unwrap()).is_some() {
                        return Err(invalid());
                    }
                }
                // gateway, multipath, via, newdst, encap, nexthop ID: never infer directness.
                5 | 9 | 18 | 19 | 21 | 22 | 30 => return Err(invalid()),
                // Known ancillary facts cannot add a gateway or select another
                // nexthop. Future route attributes are unsupported until reviewed.
                6 | 8 | 12 | 15 | 23 | 24 | 25 => {}
                7 if value == source.octets() => {} // RTA_PREFSRC
                _ => return Err(invalid()),
            }
        }
        if dst != Some(destination.octets())
            || src != Some(source.octets())
            || oif != Some(index.to_ne_bytes())
        {
            return Err(invalid());
        }
        Ok(DirectRoute {
            destination,
            source,
            index,
        })
    }
    pub(super) fn direct_route(
        source: Ipv4Addr,
        destination: Ipv4Addr,
        index: u32,
        deadline: Instant,
    ) -> io::Result<DirectRoute> {
        let mut link = [0; 16];
        link[4..8].copy_from_slice(&index.to_ne_bytes());
        check_link(&query(18, 16, &link, deadline)?, index)?; // GETLINK / NEWLINK
        let mut request = vec![libc::AF_INET as u8, 32, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        add_attr(&mut request, 1, &destination.octets());
        add_attr(&mut request, 2, &source.octets());
        // Do not set RTA_OIF: that would force the desired interface instead of querying the actual route.
        parse_route(
            &query(26, 24, &request, deadline)?,
            source,
            destination,
            index,
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        fn route() -> Vec<u8> {
            let mut r = vec![2, 32, 32, 0, 254, 0, 0, 1, 0, 2, 0, 0];
            add_attr(&mut r, 1, &[192, 168, 1, 44]);
            add_attr(&mut r, 2, &[192, 168, 1, 120]);
            add_attr(&mut r, 4, &2u32.to_ne_bytes());
            r
        }
        fn accepts(r: &[u8]) -> bool {
            parse_route(
                r,
                Ipv4Addr::new(192, 168, 1, 120),
                Ipv4Addr::new(192, 168, 1, 44),
                2,
            )
            .is_ok()
        }
        #[test]
        fn route_requires_direct_unambiguous_exact_endpoints() {
            assert!(accepts(&route()));
            for kind in [5, 9, 18, 19, 21, 22, 30, 99] {
                let mut r = route();
                add_attr(&mut r, kind, &[1, 2, 3, 4]);
                assert!(!accepts(&r));
            }
            for at in [0, 1, 2, 3, 7, 8] {
                let mut r = route();
                r[at] = 99;
                assert!(!accepts(&r));
            }
            for kind in [1, 2, 4] {
                let mut r = route();
                add_attr(&mut r, kind, &[1, 2, 3, 4]);
                assert!(!accepts(&r));
            }
            let mut r = route();
            r[16] = 42;
            assert!(!accepts(&r));
            let mut r = route();
            *r.last_mut().unwrap() = 1;
            assert!(!accepts(&r));
            for n in 0..route().len() {
                assert!(!accepts(&route()[..n]));
            }
        }
        #[test]
        fn netlink_rejects_errors_multi_wrong_sequence_and_truncation() {
            let mut response = vec![16, 0, 0, 0, 24, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
            assert!(parse_response(&response, 24).is_ok());
            for (at, value) in [(0, 0), (0, 255), (4, 2), (6, 2), (8, 2)] {
                let mut r = response.clone();
                r[at] = value;
                assert!(parse_response(&r, 24).is_err());
            }
            response.extend_from_slice(&[0; 16]);
            assert!(parse_response(&response, 24).is_err());
        }
        #[test]
        fn link_rejects_logical_master_tunnel_and_other_owner() {
            let mut link = vec![0; 16];
            link[2..4].copy_from_slice(&libc::ARPHRD_ETHER.to_ne_bytes());
            link[4..8].copy_from_slice(&2u32.to_ne_bytes());
            link[8..12].copy_from_slice(&((libc::IFF_UP | libc::IFF_RUNNING) as u32).to_ne_bytes());
            assert!(check_link(&link, 2).is_ok());
            assert!(check_link(&link, 3).is_err());
            for kind in [18, 10, 5, 37] {
                let mut r = link.clone();
                add_attr(&mut r, kind, &9u32.to_ne_bytes());
                assert!(check_link(&r, 2).is_err());
            }
        }
        #[test]
        fn deadline_is_checked_before_any_socket() {
            assert_eq!(
                query(26, 24, &[], Instant::now() - Duration::from_secs(1))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::TimedOut
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn endpoints() -> AcceptedEndpoints {
        AcceptedEndpoints {
            local: "192.168.1.120:8888".parse().unwrap(),
            peer: "192.168.1.44:1234".parse().unwrap(),
        }
    }
    fn owner() -> Interface {
        Interface {
            index: 2,
            address: Ipv4Addr::new(192, 168, 1, 120),
            mask: Ipv4Addr::new(255, 255, 255, 0),
            mac: [2, 1, 2, 3, 4, 5],
            eligible: true,
        }
    }
    fn route() -> DirectRoute {
        DirectRoute {
            destination: Ipv4Addr::new(192, 168, 1, 44),
            source: owner().address,
            index: 2,
        }
    }
    #[test]
    fn unique_owner_and_exact_connection_are_required() {
        assert_eq!(
            admit(endpoints(), &[owner()], route()).unwrap().mac,
            "02:01:02:03:04:05"
        );
        assert!(admit(endpoints(), &[], route()).is_none());
        assert!(admit(endpoints(), &[owner(), owner()], route()).is_some());
        let mut other = owner();
        other.index = 3;
        assert!(admit(endpoints(), &[owner(), other], route()).is_none());
        let mut alias = owner();
        alias.address = Ipv4Addr::new(192, 168, 1, 121);
        assert!(admit(endpoints(), &[owner(), alias], route()).is_some());
        let mut r = route();
        r.index = 3;
        assert!(admit(endpoints(), &[owner()], r).is_none());
        r = route();
        r.destination = Ipv4Addr::new(192, 168, 1, 45);
        assert!(admit(endpoints(), &[owner()], r).is_none());
        for ip in [
            "127.0.0.1:1",
            "0.0.0.0:1",
            "224.0.0.1:1",
            "192.168.2.44:1",
            "192.168.1.0:1",
            "192.168.1.255:1",
            "[::1]:1",
        ] {
            let mut e = endpoints();
            e.peer = ip.parse().unwrap();
            assert!(admit(e, &[owner()], route()).is_none());
            e = endpoints();
            e.local = ip.parse().unwrap();
            assert!(admit(e, &[owner()], route()).is_none());
        }
    }
    #[test]
    fn masks_mac_and_down_links_are_conservative() {
        for mask in [
            [0, 0, 0, 0],
            [255, 0, 255, 0],
            [255, 255, 255, 254],
            [255, 255, 255, 255],
        ] {
            let mut o = owner();
            o.mask = mask.into();
            assert!(admit(endpoints(), &[o], route()).is_none());
        }
        for mac in [[0; 6], [255; 6], [1, 2, 3, 4, 5, 6]] {
            let mut o = owner();
            o.mac = mac;
            assert!(admit(endpoints(), &[o], route()).is_none());
        }
        let mut o = owner();
        o.eligible = false;
        assert!(admit(endpoints(), &[o], route()).is_none());
    }
    #[test]
    fn old_and_new_welcome_wire_roundtrip() {
        use crate::protocol::{encode, Decoder, WireMessage};
        let old = r#"{"type":"welcome","data":{"incarnation":7,"session_epoch":9}}"#;
        let welcome: WireMessage = serde_json::from_str(old).unwrap();
        assert_eq!(serde_json::to_string(&welcome).unwrap(), old);
        for hint in [None, admit(endpoints(), &[owner()], route())] {
            let welcome = WireMessage::Welcome {
                incarnation: 7,
                session_epoch: 9,
                media_input_version: None,
                server_link: hint,
            };
            assert_eq!(
                Decoder::default().push(&encode(&welcome).unwrap()).unwrap(),
                vec![welcome]
            );
        }
    }

    #[test]
    fn stale_results_are_not_publishable() {
        let hint = admit(endpoints(), &[owner()], route()).unwrap();
        assert!(FreshHint {
            hint,
            deadline: Instant::now() - Duration::from_secs(1)
        }
        .into_current()
        .is_none());
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "explicit read-only host NIC/route test, endpoints required"]
    fn live_linux_lookup() {
        let endpoints = AcceptedEndpoints {
            local: std::env::var("PICOMADO_LINK_TEST_LOCAL")
                .expect("local socket")
                .parse()
                .unwrap(),
            peer: std::env::var("PICOMADO_LINK_TEST_PEER")
                .expect("peer socket")
                .parse()
                .unwrap(),
        };
        let facts = linux::interfaces().expect("getifaddrs");
        let (local, peer) = endpoints.ipv4().unwrap();
        let owner = facts.iter().find(|i| i.address == local).expect("owner");
        let route = linux::direct_route(local, peer, owner.index, Instant::now() + LOOKUP_BUDGET)
            .expect("direct route");
        assert!(admit(endpoints, &facts, route).is_some());
        let hint = lookup_sync(endpoints, Instant::now() + LOOKUP_BUDGET).expect("fresh hint");
        eprintln!(
            "server_link_read_only_lookup={}",
            serde_json::to_string(&hint).unwrap()
        );
    }
    #[tokio::test]
    async fn timeout_omits_hint_and_retains_bounded_worker_slot_until_exit() {
        static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
        let (release, wait) = std::sync::mpsc::channel();
        let (entered, started) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(lookup_bounded(
            endpoints(),
            Duration::from_millis(20),
            &SLOTS,
            move |_, _| {
                let _ = entered.send(());
                // Bounded fixture delay; timeout must not release this worker's slot.
                let _ = wait.recv_timeout(Duration::from_secs(1));
                admit(endpoints(), &[owner()], route())
            },
        ));
        started.await.unwrap();
        assert!(task.await.unwrap().is_none());
        assert!(
            lookup_bounded(endpoints(), Duration::from_secs(1), &SLOTS, |_, _| panic!(
                "saturated worker must be omitted"
            ))
            .await
            .is_none()
        );
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while SLOTS.available_permits() != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(
            lookup_bounded(endpoints(), Duration::from_secs(1), &SLOTS, |_, _| None)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn unsupported_endpoints_omit_hint() {
        assert!(lookup(AcceptedEndpoints {
            local: "[::1]:1".parse().unwrap(),
            peer: "[::1]:2".parse().unwrap()
        })
        .await
        .is_none());
    }
}
