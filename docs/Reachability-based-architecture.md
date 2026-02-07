# Reachability-Based Architecture

This document outlines the **Reachability-Based Architecture** adopted by the Chiral Network. This approach simplifies network topology by classifying nodes based on their proven network capabilities rather than attempting to artificially extend connectivity to all nodes via expensive relaying.

## Core Philosophy

> "If you are behind a NAT and cannot be reached, you are a Client. If you are publicly reachable, you are a Node."

Instead of using complex, bandwidth-heavy relay servers to allow NATed nodes to participate as full peers, we adopt a strict classification system. All nodes start as consumers (DHT Clients) and must "prove" their public reachability to become infrastructure providers (DHT Servers).

## Node Roles

### 1. The Observer (DHT Client)
*   **Default State:** Every node initiates in this state.
*   **Capabilities:**
    *   Can query the DHT (find peers, look up file hashes).
    *   Can download files from public nodes.
    *   **Cannot** store DHT records (key-value pairs) for the network.
    *   **Cannot** serve as a routing hop for other peers.
    *   **Cannot** be dialed directly by others (invisible to the routing table).
*   **Target Audience:** Users behind symmetric NATs, mobile networks, or restrictive firewalls without UPnP.

### 2. The Participant (DHT Server)
*   **Upgraded State:** Achieved only after passing reachability tests.
*   **Capabilities:**
    *   Full DHT participation.
    *   Stores and serves DHT records (provider records, file metadata).
    *   Acts as a routing hop in the Kademlia DHT.
    *   Can be dialed directly by any other node.
    *   Can seed files to any node (Client or Server).
*   **Target Audience:** Users with public IPs, successful port forwarding, or working UPnP.

## The Lifecycle: "The Prove-It Protocol"

The transition between roles is automated and dynamic, governed by the `AutoNAT` protocol.

### Phase 1: Initialization
1.  Node starts up.
2.  `libp2p` Kademlia behavior is initialized in **`Mode::Client`**.
3.  The node connects to bootstrap nodes.
4.  It can immediately search and download, but it does not advertise itself.

### Phase 2: The Audit (AutoNAT)
1.  The node's `AutoNAT` service periodically sends "Dial Me" requests to random peers.
2.  Peers attempt to dial the node back on its observed IP and port.
3.  **Success:** If peers successfully connect, they report "Public."
4.  **Failure:** If peers fail to connect (timeout/refused), they report "Private."

### Phase 3: Promotion or Stagnation
*   **If Public:**
    *   The node fires a `NatStatus::Public` event.
    *   Logic triggers `kademlia.set_mode(Mode::Server)`.
    *   The node enters the DHT routing table and begins answering queries.
*   **If Private:**
    *   The node remains in `Mode::Client`.
    *   It continues to function purely as a consumer.
    *   (Optional) The node attempts UPnP/IGD to punch a hole. If that succeeds later, it returns to Phase 2.

## Implications & Trade-offs

### Advantages
1.  **Network Health:** The DHT routing table is not polluted with unreachable nodes. Every node in the routing table is guaranteed to be dialable.
2.  **Resource Efficiency:** We eliminate the massive bandwidth cost of relaying traffic for NATed nodes.
3.  **Simplicity:** Reduces code complexity regarding hole-punching coordination and relay circuit management.
4.  **Incentive Compatibility:** Users who want to earn reputation or credits (by seeding) are incentivized to configure their network (port forward) properly.

### Disadvantages
1.  **Leecher Ratio:** A significant portion of the network (mobile/residential NATs) will be read-only consumers.
2.  **Reduced Seeding Pool:** NATed users cannot easily seed files to other NATed users (p2p-circuit would be required). They can usually only download.

## Technical Implementation

### Configuration
*   **`kademlia.set_mode`**: Initialized to `Client`.

### Logic Flow (`src-tauri/src/dht.rs`)

```rust
// 1. Start in Client Mode
kademlia.set_mode(Some(Mode::Client));

// ... in the event loop ...

// 2. Handle AutoNAT Event
match event {
    DhtEvent::NatStatus { state: NatReachabilityState::Public, .. } => {
        // 3. Promote to Server
        info!("Node is public! Upgrading to DHT Server.");
        swarm.behaviour_mut().kademlia.set_mode(Some(Mode::Server));
    }
    DhtEvent::NatStatus { state: NatReachabilityState::Private, .. } => {
        // 4. Remain/Downgrade to Client
        info!("Node is private. Downgrading to DHT Client.");
        swarm.behaviour_mut().kademlia.set_mode(Some(Mode::Client));
    }
}
```
