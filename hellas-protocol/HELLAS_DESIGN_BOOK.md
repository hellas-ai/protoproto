# The Hellas Protocol Design Book

## Table of Contents

1. [Introduction](#1-introduction)
2. [Core Design Philosophy](#2-core-design-philosophy)
3. [The Object-Centric State Model](#3-the-object-centric-state-model)
4. [Channelized Execution and Bounded Counters](#4-channelized-execution-and-bounded-counters)
5. [Transaction Flows](#5-transaction-flows)
6. [Off-Chain Negotiation](#6-off-chain-negotiation)
7. [Security and Trust Model](#7-security-and-trust-model)
8. [Performance Analysis](#8-performance-analysis)
9. [Implementation Considerations](#9-implementation-considerations)
10. [Future Extensions](#10-future-extensions)

---

## 1. Introduction

The Hellas Protocol is a purpose-built blockchain designed specifically for decentralized AI compute marketplaces. It achieves what seems impossible: ultra-low latency for interactive AI workloads (like streaming LLM responses) while maintaining the security guarantees needed for untrusted compute providers.

### 1.1 The Problem

Traditional blockchains face a fundamental trilemma when applied to AI compute:

1. **Latency**: AI applications need sub-second response times
2. **Throughput**: Popular AI services need to handle thousands of requests per second
3. **Trust**: Compute providers might deliver incorrect results or not deliver at all

Existing solutions either:
- Use centralized coordinators (sacrificing decentralization)
- Accept high latency (making interactive use cases impossible)
- Require all computation on-chain (prohibitively expensive)

### 1.2 The Hellas Solution

Hellas resolves this trilemma through several key innovations:

1. **Dual Transaction Flows**: Separate paths for trusted (interactive) and untrusted (batch) workloads
2. **Channelized Execution**: Validators process transactions independently without coordination
3. **Object-Centric State**: Parallel execution of non-conflicting transactions
4. **Off-Chain Negotiation**: Only cryptographic commitments stored on-chain

The result is a blockchain that can process interactive AI requests in ~100ms while maintaining security for batch jobs through an escrow system.

---

## 2. Core Design Philosophy

### 2.1 State Machine, Not Virtual Machine

Unlike Ethereum or similar platforms, Hellas is **not** a general-purpose computing platform. It's a specialized state machine with a fixed set of transaction types. This decision has profound implications:

**Benefits:**
- **Performance**: No VM overhead, direct native execution
- **Security**: Smaller attack surface, easier to audit
- **Optimization**: Can aggressively optimize for known patterns
- **Determinism**: Easier to achieve consensus

**Trade-offs:**
- **Flexibility**: Cannot add new features without protocol upgrades
- **Ecosystem**: No permissionless smart contract development

This trade-off is acceptable because AI compute has well-defined patterns that rarely change.

### 2.2 Smart Clients, Dumb Protocol

The protocol provides powerful primitives but leaves complex logic to clients:

**On-Chain (Dumb Protocol):**
- Payment settlement
- Escrow management
- Cryptographic commitments
- Basic state transitions

**Off-Chain (Smart Clients):**
- Job specification and negotiation
- Provider selection algorithms
- Reputation systems
- Result verification strategies
- Job splitting and aggregation

This separation enables rapid innovation in client software without protocol changes.

### 2.3 Concurrency by Default

Every design decision prioritizes concurrent execution:

- Objects have explicit owners and versions
- Transactions declare their inputs upfront
- Bounded counters enable parallel spending
- No global state or singletons

### 2.4 Minimal On-Chain Data

The blockchain stores only what's essential for financial settlement:

```rust
// Instead of storing full job specifications:
struct Job {
    model: String,          // ❌ Hundreds of KB
    inputs: Vec<Tensor>,    // ❌ Potentially GB
    parameters: Config,     // ❌ Complex nested data
}

// We store only commitments:
struct JobEscrow {
    job_spec_hash: Hash,    // ✅ Just 32 bytes
    payment: u64,           // ✅ Essential for settlement
    result_hash: Option<Hash>, // ✅ Proof of completion
}
```

---

## 3. The Object-Centric State Model

### 3.1 Why Objects?

Traditional blockchains use an account model (like Ethereum) or UTXO model (like Bitcoin). Hellas uses an object model inspired by Sui:

**Account Model Problems:**
- Single account = bottleneck for popular services
- All transactions touching an account must be serialized
- Complex locking mechanisms needed

**Object Model Benefits:**
- Natural parallelism (different objects = no conflicts)
- Clear ownership semantics
- Version tracking prevents double-spending
- Efficient caching and sharding

### 3.2 Object Structure

Every object in Hellas has the same metadata wrapper:

```rust
pub struct ObjectMetadata {
    pub id: ObjectId,              // Unique identifier
    pub version: Version,          // Monotonic counter
    pub owner_set: HashSet<Pubkey>, // Who can modify this
    pub object: Object,            // The actual data
}
```

**Key Properties:**

1. **Immutable IDs**: Once created, an object's ID never changes
2. **Versioning**: Every modification increments the version
3. **Multi-Owner Support**: Enables joint accounts and escrows
4. **Type Safety**: The `Object` enum ensures only known types

### 3.3 Object Lifecycle

Objects follow a simple lifecycle:

1. **Creation**: Transaction creates object at version 0
2. **Modification**: Transaction consumes version N, creates N+1
3. **Deletion**: Object consumed but no new version created

This model makes state transitions explicit and auditable.

---

## 4. Channelized Execution and Bounded Counters

### 4.1 The Concurrency Challenge

The biggest bottleneck in blockchains is contention on popular accounts. Consider an AI service provider receiving thousands of payments - in a naive system, all these transactions conflict and must be processed serially.

### 4.2 The Bounded Counter Solution

Hellas implements the Bounded Counter pattern from the Stingray paper:

```rust
pub struct HellasAccount {
    pub balance: u64,  // Total funds
    pub local_validator_budgets: HashMap<Pubkey, u64>, // Per-validator allowances
}
```

Instead of all transactions modifying the balance directly, each validator has a "budget" they can spend from without coordination.

### 4.3 Byzantine Fault Tolerance

The key insight is the mathematical bound on validator budgets:

```
η = (f + 1) / (2f + 1)
```

Where `f` is the number of Byzantine validators tolerated. For example, with 3f+1 = 4 validators (f=1):
- η = 2/3
- Each validator can spend up to 2/3 of the account balance
- Even if 1 validator is Byzantine, at most 2/3 can be stolen

### 4.4 Channelized Execution Flow

```
Time →

Validator 1: [----Payment A----] [----Payment D----]
Validator 2:      [----Payment B----]      [----Payment E----]
Validator 3: [--------Payment C--------]
Validator 4:           [----Payment F----]

                                    ↓
                            [ResetBudget Transaction]
                                    ↓
                            All spending reconciled
```

Each validator processes payments independently using their channel. Periodically, a `ResetBudget` transaction aggregates all spending and redistributes budgets.

### 4.5 Implementation Details

The channelized model has three key components:

1. **Local State**: Each validator tracks spending in their channel
2. **Budget Certificates**: Cryptographic proofs of spending
3. **Reconciliation**: Periodic budget resets maintain consistency

```rust
pub struct ValidatorLocalState {
    pub validator_id: Pubkey,
    pub account_channels: HashMap<ObjectId, ChannelState>,
}

pub struct ChannelState {
    pub remaining_budget: u64,
    pub total_spent: u64,
    pub processed_txs: Vec<TransactionDigest>,
}
```

---

## 5. Transaction Flows

### 5.1 Interactive Flow (SettleDirectly)

For pre-established trust relationships (e.g., enterprise customers), Hellas provides a single-transaction settlement:

```
Client → Provider: Execute LLM inference
Provider → Client: Stream results
Client + Provider → Chain: SettleDirectly (both sign)
```

**Transaction Structure:**
```rust
Transaction::SettleDirectly {
    provider: Pubkey,
    job_spec_hash: Hash,    // What was computed
    result_hash: Hash,      // Proof of result
    payment: u64,
}
```

**Key Properties:**
- Requires signatures from both parties
- Single round-trip to chain (~100ms)
- Uses channelized execution for parallelism
- No escrow or waiting periods

### 5.2 Marketplace Flow (Escrow)

For untrusted relationships, Hellas provides a multi-step escrow flow:

```
1. PostJob:      Requestor locks payment
2. ClaimJob:     Provider locks bond
3. CommitResult: Provider posts result hash
4. FinalizeJob:  Payment released after challenge period
```

**State Machine:**
```
Posted → Claimed → Committed → Finalized
  ↓         ↓          ↓
  └─────────┴──────────┴────→ Aborted (on timeout)
```

### 5.3 Choosing Between Flows

The protocol doesn't enforce which flow to use. Clients decide based on:

- **Trust Level**: Known provider → Interactive, Unknown → Marketplace
- **Job Type**: Streaming → Interactive, Batch → Marketplace
- **Risk Tolerance**: Low risk → Interactive, High value → Marketplace

---

## 6. Off-Chain Negotiation

### 6.1 The Negotiation Protocol

Before any on-chain transaction, parties negotiate off-chain:

```
1. Requestor broadcasts JobSpec
2. Providers submit ProviderQuotes
3. Requestor selects best quote
4. Both sign JobAgreement
5. Execute appropriate on-chain flow
```

### 6.2 Data Structures

**Job Specification:**
```rust
pub struct JobSpec {
    pub catgrad_graph_hash: Hash,     // Computation to perform
    pub input_hashes: Vec<Hash>,      // Input data references
    pub requirements: JobRequirements, // Hardware needs
    pub security_params: SecurityParams,
    pub max_price: u64,
}
```

**Provider Quote:**
```rust
pub struct ProviderQuote {
    pub job_spec_hash: Hash,
    pub provider: Pubkey,
    pub price: u64,
    pub estimated_latency_ms: u64,
    pub capabilities: ProviderCapabilities,
}
```

### 6.3 Benefits of Off-Chain Negotiation

1. **Efficiency**: No failed transactions from mismatched requirements
2. **Privacy**: Negotiation details never go on-chain
3. **Flexibility**: Easy to add new negotiation parameters
4. **Scalability**: Unlimited negotiation volume

---

## 7. Security and Trust Model

### 7.1 Threat Model

Hellas assumes:

1. **Validators**: Up to f out of 3f+1 may be Byzantine
2. **Providers**: May deliver incorrect results or not deliver
3. **Requestors**: May refuse to pay for valid results
4. **Network**: Messages may be delayed but not indefinitely

### 7.2 Security Mechanisms

**For Validators:**
- BFT consensus (e.g., Tendermint, HotStuff)
- Bounded counter limits on spending
- Slashing for protocol violations

**For Providers:**
- Performance bonds (slashed on misbehavior)
- Reputation systems (client-side)
- Future: Fraud proofs and ZK verification

**For Requestors:**
- Upfront payment into escrow
- Automatic release after challenge period
- No ability to claw back confirmed payments

### 7.3 Trust Assumptions

**Interactive Flow requires:**
- Mutual trust between specific client and provider
- Trust that provider won't double-spend across validators

**Marketplace Flow requires:**
- Only trust in the validator set (not counterparty)
- Trust in challenge period mechanism

---

## 8. Performance Analysis

### 8.1 Throughput

**Theoretical Maximum:**
- Assume 4 validators, 1000 transactions per block
- With perfect parallelism: 4000 TPS per object
- With bounded counters: ~3000 TPS per account (due to η factor)

**Practical Expectations:**
- Network and consensus overhead: ~50%
- Real-world parallelism: ~70%
- Expected: 1000-2000 TPS globally

### 8.2 Latency

**Interactive Flow:**
```
Client signing:        ~1ms
Network round-trip:   ~50ms
Consensus:           ~50ms
Execution:            ~1ms
Total:              ~102ms
```

**Marketplace Flow:**
```
PostJob:         ~100ms
ClaimJob:        ~100ms (after finding job)
CommitResult:    ~100ms (after computation)
FinalizeJob:     ~100ms (after challenge period)
Total active:    ~300ms (plus waiting times)
```

### 8.3 Scalability Paths

1. **Horizontal**: Add more validators (increases throughput)
2. **Sharding**: Partition objects across validator groups
3. **Layer 2**: Rollups for specific use cases
4. **Optimizations**: Better parallel execution algorithms

---

## 9. Implementation Considerations

### 9.1 Consensus Integration

Hellas is consensus-agnostic but requires:
- Total ordering of transactions
- Byzantine fault tolerance
- Reasonable finality times (~1-2 seconds)

Suitable options:
- Tendermint/CometBFT
- HotStuff/LibraBFT
- Narwhal/Bullshark

### 9.2 Storage Requirements

**Per Object:**
- Metadata: ~100 bytes
- Account: ~1KB (with many validator budgets)
- Escrow: ~500 bytes

**Growth Rate:**
- Assume 1000 TPS, 50% are payments
- ~43M transactions per day
- ~50GB per day (with proofs and indices)
- ~18TB per year

### 9.3 Network Requirements

**Bandwidth:**
- Transaction: ~500 bytes
- Block header: ~1KB
- Signatures: ~64 bytes each

At 1000 TPS with 4 validators:
- ~2MB/s per validator
- ~20Mbps sustained bandwidth

### 9.4 Client Implementation

Clients need to:
1. Track object versions
2. Manage nonces
3. Handle channel selection for payments
4. Implement retry logic
5. Monitor for ResetBudget opportunities

---

## 10. Future Extensions

### 10.1 Fraud Proof System

The current design supports but doesn't implement fraud proofs:

```rust
enum DisputeTransaction {
    InitiateDispute { escrow_id: ObjectId, claim: FraudClaim },
    RespondToDispute { dispute_id: ObjectId, evidence: Hash },
    ResolveDispute { dispute_id: ObjectId, verdict: Verdict },
}
```

### 10.2 Zero-Knowledge Verification

Ultimate dispute resolution via ZK proofs:
- Provider generates ZK proof of correct execution
- Only used when disputed (expensive but final)
- Chain verifies proof and distributes funds

### 10.3 Cross-Chain Bridges

Enable payment from other chains:
- Lock tokens on Ethereum/Solana
- Mint wrapped tokens on Hellas
- Execute compute transactions
- Burn and unlock on exit

### 10.4 Advanced Scheduling

Smart routing of jobs to providers:
- On-chain provider registry
- Capability advertisements
- Automatic matching algorithms
- Load balancing primitives

### 10.5 Collective Objects

From Stingray paper - objects with multiple concurrent writers:
- Version vectors instead of single versions
- Merge operations for conflicts
- Useful for shared model registries

---

## Conclusion

The Hellas Protocol represents a new approach to blockchain design: rather than building a general-purpose platform and trying to optimize it, we started with specific requirements (AI compute marketplace) and built the minimal system that serves those needs well.

Key takeaways:

1. **Specialization Enables Performance**: By limiting scope, we achieve 100ms latency
2. **Concurrency is Essential**: Channelized execution and object model enable scale
3. **Off-Chain Negotiation**: Keeps the chain lean and efficient
4. **Dual Flows**: Different trust models need different solutions
5. **Future-Proof**: Extensible for fraud proofs and ZK verification

The result is a blockchain that can actually support real-time AI applications while maintaining the security properties that make blockchains valuable. Hellas proves that with careful design, we can have both speed and security.