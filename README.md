# "lockstep"

This is a personal learning experiment.
The end goal is a deterministic multiplayer simulation running reliably with low latency and bandwidth usage.
Despite the name, we may or may not end up actually using the lockstep technique.

## Networking Constraints

TCP is inherently slow under packet loss, by design, because of head-of-line blocking and avoiding retransmission until at least a full roundtrip to the peer, leading to at least 1.5 times the RoundTrip Time (RTT) latency per packet (e.g. 200ms RTT -> at least 300ms latency under packet loss, unacceptable for games!). Disabling Nagle's algorithm helps against head-of-line blocking but there exists no mitigation against latency under packet loss.

> 1. We have to drop lower than TCP, down to UDP, in order to decrease latency under packet loss.

However, UDP is connection-less, all we have are packets.

> 2. We must build our own virtual connection system.

Also, UDP has no acknowledgment mechanism and no re-transmission of lost packets.

> 3. We must build our own reliable packet delivery system.

Further, with UDP, packets could arrive out of order.

> 4. We must build our own packet ordering system.

Packets are limited in size by the PMTU (Path Maximum Translation Unit). The supremum (= least upper bound) for PMTU is 576B for IPv4 and 1280B for IPv6; however, we must subtract the size of the 8B UDP and 20B IP headers, reaching 548B for IPv4 and 1252B for IPv6. PMTU discovery exists, but using it is complicated because network topology may change over time, wherefore it's hard to guarantee that a certain PMTU will not shrink after measurements are made: in the worst case, routers will drop packets!

> 5. We must send packets of maximum size 548B for IPv4 and 1252B for IPv6.

## Design Choices

1. A dedicated UDP server accepts N clients to connect simultaneously.

## Milestones

### Milestone 1 - Network Proof of Concept (PoC)

A basic low-latency reliable ordered protocol with virtual connections over UDP

- [x] connect clients with a server using UDP
- [x] run in non-blocking (polled) mode, at some frequency
- [x] simple packet filter
  - [x] check expected packet size
  - [x] match crc32 checksums including custom protocol identifier
  - [x] assert against buffer overflows on read and write
  - [x] limit buffer size under PMTU supremum (IPv4: 548B, IPv6: 1252B)
  - [x] client: check server address
  - [x] server: check client address
- [x] serialization
  - [x] generic implementation of read/write with endianness handling
  - [x] uniform read/write path to decrease manual errors
- [x] virtual connection
  - [x] simple connection request/accept/deny
  - [x] detect and handle connectivity loss (timeout)
  - [x] automatic keep-alive packets
- [x] reliability and ordering
  - [x] sequence numbers, acks, ack bitfield
  - [x] queue packets for later send, (re)send unacked on every network frame
  - [x] store read messages in ordered queue and poll out in order
  - [x] zero runtime buffer allocations
- [x] stats
  - [x] round-trip time (=rtt)
  - [x] bytes sent per frame (excluding UDP+IP headers)
- [x] ease of use
  - [x] semi-uniform interface for server and client with low amount of boilerplate
  - [x] only expose user data, not protocol packets
  - [x] abstract away the streaming of buffers in/out of structures

### Milestone 2 - Interactive Lockstep Simulation PoC

- [x] lobby state sync with connects / disconnects
- [x] synchronized startup
- [ ] handle mid-game disconnects
- [ ] use winit and gather user input
- [ ] run simulation from user input
- [ ] save/load simulation state
- [ ] replay simulation from file
- [ ] network-adapting simulation latency
- [ ] reconnects

### Milestone X - Polish

- [ ] whisper (relay chat message via server)
- [ ] lobby chat
- [ ] configuration: file / environment variables / command line arguments
- [ ] clippy configuration
- [ ] bitpacking and efficient serialization
- [ ] security hardening
- [ ] handle network topology changes (client IP could change)
- [ ] detect and handle congestion?
  - [ ] limit sends per network frame so not too many unacked packets are resent
  - [ ] 250ms max average rtt
  - [ ] on >= rtt max, go to bad state
  - [ ] after t time, go back to good state
  - [ ] on edge to bad after <10s in good state, set t=min(t\*2, 60)
  - [ ] on >= 10s in good state, set t=max(t/2, 1)
  - [ ] send frequency: 30hz good / 10hz bad
