# Morpheus Consensus:
# Excelling on trails and autobahns

**ANDREW LEWIS-PYE**, London School of Economics, UK
**EHUD SHAPIRO**, London School of Economics, UK and Weizmann Institute of Science, Israel

Recent research in consensus has often focussed on protocols for State-Machine-Replication (SMR) that can handle high throughputs. Such state-of-the-art protocols (generally DAG-based) induce undue overhead when the needed throughput is low, or else exhibit unnecessarily-poor latency and communication complexity during periods of low throughput.

Here we present Morpheus Consensus, which naturally morphs from a quiescent low-throughput leaderless blockchain protocol to a high-throughput leader-based DAG protocol and back, excelling in latency and complexity in both settings. During high-throughout, Morpheus pars with state-of-the-art DAG-based protocols, including Autobahn [15]. During low-throughput, Morpheus exhibits competitive complexity and lower latency than standard protocols such as PBFT [10] and Tendermint [8, 9], which in turn do not perform well during high-throughput.

The key idea of Morpheus is that as long as blocks do not conflict (due to Byzantine behaviour, network delays, or high-throughput simultaneous production) it produces a forkless blockchain, promptly finalizing each block upon arrival. It assigns a leader only if one is needed to resolve conflicts, in a manner and with performance not unlike Autobahn.

## 1 INTRODUCTION

Significant investment in blockchain technology has recently led to renewed interest in research on consensus protocols. Much of this research is focussed on developing protocols that operate efficiently 'at scale'. In concrete terms, this means looking to design protocols that can handle a high throughput (i.e. high rate of incoming transactions) with low latency (i.e. quick transaction finalization), even when the number of processes (validators) carrying out the protocol is large.

**Dealing efficiently with low and high throughput.** While blockchains may often need to handle high throughputs, it is not the case that all blockchains need to deal with high throughput all of the time. For example, various 'subnets' or 'subchains' may only have to deal with high throughputs infrequently, and should ideally be optimised to deal also with periods of low throughput. The motivation for the present paper therefore stems from a real-world need for consensus protocols that deal efficiently with both high and low throughputs. Specifically, we are interested in a setting where:

(1) The processes/validators may be few, but could be up to a few hundred in number.
(2) The protocol should be able to handle periods of asynchrony, i.e. should operate efficiently in the partially synchronous setting.
(3) The protocol is required to have optimal resilience against Byzantine adversaries, i.e. should be live and consistent so long as less than 1/3 of processes display Byzantine faults, but should be optimised to deal with the 'normal case' that processes are not carrying out Byzantine attacks and that faults are benign (crash or omission failures).
(4) There are expected to be some periods of high throughput, meaning that the protocol should ideally match the state-of-the-art during such periods.
(5) Often, however, throughput will be low. This means the protocol should also be optimised to give the lowest possible latency during periods of low throughput.
(6) Ideally, the protocol should be 'leaderless' during periods of low throughput: the use of leaders is to be avoided if possible, since leaders who are offline/faulty may cause significant increases in latency.
(7) Ideally, the protocol should also be 'quiescent', i.e. there should be no need for the sending and storing of new messages when new transactions are not being produced.
(8) Transactions may come from clients (not belonging to the list of processes/validators), but will generally be produced by the processes themselves.

**The main contribution of this paper.** We introduce and analyse the Morpheus protocol, which is designed for the setting described above. The protocol is quiescent and has the following properties during periods of low throughput:

* It is leaderless, in the sense that transactions are finalized without the requirement for involvement by leaders.
* Transactions are finalized in time 3δ, where δ is the actual (and unknown) bound on message delays after GST. As explained in Section 7, this more than halves the latency of existing DAG-based protocols and variants such as Autobahn [15] for the low throughput case, and even decreases latency by at least δ when compared with protocols such as PBFT (and even if we suppose leaders for those protocols are non-faulty), since the leaderless property of our protocol negates the need to send transactions to a leader before they can be included in a block.
* A further advantage over protocols such as PBFT and Tendermint is that crash failures by leaders are not able to impact latency during periods of low throughput.

During periods of high throughput, Morpheus is very similar to Autobahn, and so inherits the benefits of that protocol. In particular:

* It has the same capability to deal with high throughput as DAG-based protocols and variants such as Autobahn, and has the same ability to recover quickly from periods of asynchrony ('seamless recover' in the language of Autobahn).
* It has the same latency as Autobahn during high throughput, matching the latency of Sailfish [23], which is the most competitive existing DAG-based protocol in terms of latency.
* As detailed in Section 7, Morpheus has the same advantages as Autobahn in terms of communication complexity when compared to DAG-based protocols such as Sailfish, DAG-Rider [17], Cordial Miners [18], Mysticeti [3] or Shoal [25].

Of course, much of the complexity in designing a protocol that operates efficiently in both low and high throughput settings is to ensure a smooth transition and consistency between the different modes of operation that the two settings necessitate.

**Further contributions of the paper.** In Section 3, we also formalise the task of Extractable SMR, as an attempt to make explicit certain implicit assumptions that are often made in papers in the area. While State-Machine-Replication (SMR) requires correct processes to finalize logs (sequences of transactions) in such a way that consistency and liveness are satisfied, many papers describing protocols for SMR specify protocols that do not actually suffice to ensure all correct processes receive all finalized blocks (required for liveness). Roughly, the protocol instructions suffice instead to ensure data availability (that each finalized block is received by at least one correct process), and then the protocol is required to establish a total ordering on transactions that can be extracted via further message exchange, given data availability. Liveness is therefore only achieved after further message exchange (and via some unspecified method), which is not generally taken into account when calculating message complexity.

In Hotstuff [28], for example, one of the principal aims is to ensure linear message complexity within views. Since this precludes all-to-all communication within views, a Byzantine leader may finalize a block of transactions in a given view without certain correct processes even receiving the block. Those correct processes must eventually receive the block for liveness to be satisfied, but the protocol instructions do not explicitly stipulate the mechanism by which this should be achieved, and the messages required to do so are not counted when analyzing message complexity.

The question arises, "what precisely is the task being achieved by such protocols if they do not satisfy liveness without further message exchange (and so actually fail to achieve the task of SMR with the communication complexity computed)". We assert that the task of Extractable SMR is an appropriate formalisation of the task being achieved, and hope that the introduction of this notion is a contribution of independent interest.

**The structure of the paper.** The paper structure is as follows:

* Section 2 describes the basic model and definitions.
* Section 3 formalises the task of Extractable SMR.
* Section 4 gives the intuition behind the Morpheus protocol.
* Section 5 gives the formal specification of the protocol.
* Section 6 formally establishes consistency and liveness.
* Section 7 analyses communication complexity and latency, and makes comparisons with the state-of-the-art.
* Section 8 discusses related work.

## 2 THE SETUP

We consider a set Π = {p₀, ..., pₙ₋₁} of n processes. Each process pᵢ is told i as part of its input. We consider an adaptive adversary, which chooses a set of at most f processes to corrupt during the execution, where f is the largest integer less than n/3. A process that is corrupted by the adversary is referred to as Byzantine and may behave arbitrarily, subject to our cryptographic assumptions (stated below). Processes that are not Byzantine are correct.

**Cryptographic assumptions.** Our cryptographic assumptions are standard for papers in distributed computing. Processes communicate by point-to-point authenticated channels. We use a cryptographic signature scheme, a public key infrastructure (PKI) to validate signatures, a threshold signature scheme [6, 22], and a cryptographic hash function H. The threshold signature scheme is used to create a compact signature of m-of-n processes, as in other consensus protocols [29]. In this paper, m = n−f or m = f+1. The size of a threshold signature is O(κ), where κ is a security parameter, and does not depend on m or n. We assume a computationally bounded adversary. Following a common standard in distributed computing and for simplicity of presentation (to avoid the analysis of negligible error probabilities), we assume these cryptographic schemes are perfect, i.e. we restrict attention to executions in which the adversary is unable to break these cryptographic schemes. Hash values are thus assumed to be unique.

**Message delays.** We consider a discrete sequence of timeslots t ∈ ℕ≥₀ in the partially synchronous setting: for some known bound Δ and unknown Global Stabilization Time (GST), a message sent at time t must arrive by time max{GST, t} + Δ. The adversary chooses GST and also message delivery times, subject to the constraints already specified. We write δ to denote the actual (unknown) bound on message delays after GST, noting that δ may be significantly less than the known bound Δ.

**Clock synchronization.** We do not suppose that the clocks of correct processes are synchronized. For the sake of simplicity, however, we do suppose that the clocks of correct processes all proceed in real time, i.e. if t′ > t then the local clock of correct p at time t′ is t′ − t in advance of its value at time t. This assumption is made only for the sake of simplicity, and our arguments are easily adapted to deal with a setting in which there is a known upper bound on the difference between the clock speeds of correct processes after GST. We suppose all correct processes begin the protocol execution before GST. A correct process may begin the protocol execution with its local clock set to any value.

**Transactions.** Transactions are messages of a distinguished form. For the sake of simplicity, we consider a setup in which each process produces their own transactions, but one could also adapt the presentation to a setup in which transactions are produced by clients who may pass transactions to multiple processes.

## 3 EXTRACTABLE SMR

**Informal discussion.** State-Machine-Replication (SMR) requires correct processes to finalize logs (sequences of transactions) in such a way that consistency and liveness are satisfied. As noted in Section 1, however, for many papers describing protocols for SMR, the explicit instructions of the protocol do not actually suffice to ensure liveness without further message exchange, potentially impacting calculations of message complexity and other measures. Roughly, the protocol instructions do not explicitly ensure that all correct processes receive all finalized blocks, but rather ensure data availability (that each finalized block is received by at least one correct process), and then the protocol is required to establish a total ordering on transactions that can be extracted via further message exchange, given data availability. Although it is clear that the protocol can be used to solve SMR given some (as yet unspecified) mechanism for message exchange, the protocol itself does not solve SMR. So, what exactly is the task that the protocol solves?

**Extractable SMR (formal definition).** If σ and τ are strings, we write σ ⊆ τ to denote that σ is a prefix of τ. We say σ and τ are compatible if σ ⊆ τ or τ ⊆ σ. If two strings are not compatible, they are incompatible. If σ is a sequence of transactions, we write tr ∈ σ to denote that the transaction tr belongs to the sequence σ.

If P is a protocol for extractable SMR, then it must specify a function F that maps any set of messages to a sequence of transactions. Let M* be the set of all messages that are received by at least one (potentially Byzantine) process during the execution. For any timeslot t, let M(t) be the set of all messages that are received by at least one correct process at a timeslot ≤ t. We require the following conditions to hold:

**Consistency.** For any M₁ and M₂, if M₁ ⊆ M₂ ⊆ M*, then F(M₁) ⊆ F(M₂).

**Liveness.** If correct p produces the transaction tr, there must exist t such that tr ∈ F(M(t)).

Note that consistency suffices to ensure that, for arbitrary M₁, M₂ ⊆ M*, F(M₁) and F(M₂) are compatible. To see this, note that, by consistency, F(M₁) ⊆ F(M₁ ∪ M₂) and F(M₂) ⊆ F(M₁ ∪ M₂).

**Converting protocols for Extractable SMR to protocols for SMR.** In this paper, we focus on the task of Extractable SMR. One way to convert a protocol for Extractable SMR into a protocol for SMR is to assume the existence of a gossip network, in which each process has some (appropriately chosen) constant number of neighbors. Using standard results from graph theory ([5] Chapter 7), one can assume correct processes form a connected component: this assumption requires classifying some small number of disconnected processes that would otherwise be correct as Byzantine. If each correct process gossips each 'relevant' protocol message, then all such messages will eventually be received by all correct processes. Overall, this induces an extra communication cost per message which is only linear in n. Of course, other approaches are also possible, and in this paper we will remain agnostic as to the precise process by which SMR is achieved from Extractable SMR.

## 4 MORPHEUS: THE INTUITION

In this section, we informally describe the intuition behind the protocol. The protocol may be described as 'DAG-based', in the sense that each block may point to more than one previous block via the use of hash pointers. The blocks observed by any block b are b and all those blocks observed by blocks that b points to. The set of blocks observed by b is denoted [b]. If neither of b and b′ observe each other, then these two blocks are said to conflict. Blocks will be of three kinds: there exists a unique genesis block bₐ (which observes only itself), and all other blocks are either transaction blocks or leader blocks.

**The operation during low throughput.** Roughly, by the 'low throughput mode', we mean a setting in which processes produce blocks of transactions infrequently enough that correct processes agree on the order in which they are received, meaning that transaction blocks can be finalized individually upon arrival. Our aim is to describe a protocol that finalizes transaction blocks with low latency in this setting, and without the use of a leader: the use of leaders is to be avoided if possible, since leaders who are offline/faulty may cause significant increases in latency. The way in which Morpheus operates in this setting is simple:

(1) Upon having a new transaction block b to issue, a process pᵢ will send b to all processes.
(2) If they have not seen any blocks conflicting with b, other processes then send a 1-vote for b to all processes.
(3) Upon receiving n−f 1-votes for b, and if they still have not seen any block conflicting with b, each correct process will send a 2-vote for b to all others.
(4) Upon receiving n−f 2-votes for b, a process regards b as finalized.

Recall that δ is the actual (unknown) bound on message delays after GST. If the new transaction block b is created at time t > GST, then the procedure above causes all correct processes to regard b as finalized by time t + 3δ.

Which blocks should a new transaction block b point to? For the sake of concreteness, let us specify that if there is a sole tip amongst the blocks received by pᵢ, i.e. if there exists a unique block b′ amongst those received by pᵢ which observes all other blocks received by pᵢ, then pᵢ should have b point to b′. To integrate with our approach to the 'high throughput mode', we also require that b should point to the last transaction block created by pᵢ. Generally, we will only require transaction blocks to point to at most two previous blocks. This avoids the downside of many DAG-based protocols that all blocks require O(n) pointers to previous blocks.

**Moving to high throughput.** When conflicting transaction blocks are produced, we need a method for ordering them. The approach we take is to use leaders, who produce a second type of block, called leader blocks. These leader blocks are used to specify the required total ordering.

**Views.** In more detail, the instructions for the protocol are divided into views, each with a distinct leader. If a particular view is operating in 'low throughput' mode and conflicting blocks are produced, then some time may pass during which a new transaction block fails to be finalized. In this case, correct processes will complain, by sending messages indicating that they wish to move to the next view. Once processes enter the next view, the leader of that view will then continue to produce leader blocks so long as the protocol remains in high throughput mode. Each of these leader blocks will point to all tips (i.e. all blocks which are not observed by any others) seen by the leader, and will suffice to specify a total ordering on the blocks they observe.

**The two phases of a view.** Each view is thus of potentially unbounded length and consists of two phases. During the first phase, the protocol in high throughput mode, and is essentially the same as Autobahn. Processes produce transaction blocks, each of which just points to their last produced transaction block. Processes do not send 1 or 2-votes for transaction blocks during this phase, but rather vote for leader blocks, which, when finalized, suffice to specify the required total ordering on transactions. Leader blocks are finalized as in PBFT, after two rounds of voting. If a time is reached after which transaction blocks arrive infrequently enough that leader blocks are no longer required, then the view enters a second phase, during which processes vote on transaction blocks and attempt to finalize them without the use of a leader.

**How to produce the total ordering.** For protocols in which each block points to a single precedessor, the total ordering of transactions specified by a finalized block b is clear: the ordering on transactions is just that inherited by the sequence of blocks below b and the transactions they contain. In a context where each block may point to multiple others, however, we have extra work to do to specify the required total ordering on transactions. The approach we take is similar to many DAG-based protocols (e.g. [18]). Given any sequence of blocks S, we let Tr(S) be the corresponding sequence of transactions, i.e. if b₁, ..., bₖ is the subsequence of S consisting of the transaction blocks in S, then Tr(B) is b₁.Tr * b₂.Tr * ... * bₖ.Tr, where * denotes concatenation, and where b.Tr is the sequence of transaction in b. We suppose given τ† such that, for any set of blocks B, τ†(B) is a sequence of blocks that contains each block in B precisely once, and which respects the observes relation: if b, b′ ∈ B and b′ observes b, then b appears before b′ in τ†(B). Each transaction/leader block b will contain q which is a 1-QC (i.e. a threshold signature formed from n−f 1-votes) for some previous block: this will be recorded as the value b.1-QC = q, while, if q is a 1-QC for b′, then we set q.b = b′. QCs are ordered first by the view of the block to which they correspond, then by the type of the block (leader or transaction, with the latter being greater), and then by the height of the block. We then define τ(b) by recursion:

* τ(bₐ) = bₐ.
* If b ≠ bₐ, then let q = b.1-QC and set b′ = q.b. Then τ(b) = τ(b′) * τ†([b] − [b′]).

Given any set of messages M, let M′ be the largest set of blocks in M that is downward closed, i.e. such that if b ∈ M′ and b observes b′, then b′ ∈ M′. Let q be a maximal 2-QC in M such that q.b ∈ M′, and set b = q.b, or if there is no such 2-QC in M, set b = bₐ. We define F(M) to be Tr(τ(b)).

**Maintaining consistency.** Consistency is formally established in Section 6, and uses a combination of techniques from PBFT, Tendermint, and previous DAG-based protocols. Roughly, the argument is as follows. When the protocol moves to a new view, consistency will be maintained using the same technique as in PBFT. Upon entering the view, each process sends a 'new-view' message to the leader, specifying the greatest 1-QC they have seen. Upon producing a first leader block b for the view, the leader must then justify the choice of b.1-QC by listing new-view messages signed by n−f distinct processes in Π. The value b.1-QC must be greater than or equal to all 1-QCs specified in those new-view messages. If any previous block b′ has received a 2-QC, then at least f+1 correct processes must have seen a 1-QC for b′, meaning that b.1-QC must be greater than or equal to that 1-QC. Subsequent leader blocks b″ for the view just set b″.1-QC to be a 1-QC for the previous leader block.

![Fig. 1. Specifying τ to produce the total ordering](image_not_included)

To maintain consistency between finalized transaction blocks and between leader and transaction blocks within a single view, we also have each transaction block specify q which is 1-QC for some previous block. Correct processes will not vote for the transaction block unless q is greater than or equal to any 1-QC they have previously received.

Overall, the result of these considerations is that, if two blocks b and b′ receive 2-QCs q and q′ respectively, with q greater than q′, then the iteration specifying τ(b) (as detailed above) proceeds via b′, so that τ(b) extends τ(b′).

**0-votes.** While operating in low throughput, a 1-QC for a block b suffices to ensure both data availability, i.e. that some correct process has received the block, and non-equivocation, i.e. two conflicting blocks cannot both receive 1-QCs. When operating in high throughput, however, transaction blocks will not receive 1 or 2-votes. In this context, we still wish to ensure data availability. It is also useful to ensure that each individual process does not produce transaction blocks that conflict with each other, so as to bound the number of tips that may be created. To this end, we make use of 0-votes, which may be regarded as weaker than standard votes for a block:

(1) Upon having a new transaction block b to issue, a process pᵢ will send b to all processes.
(2) If the block is properly formed, and if other processes have not seen pᵢ produce any transaction blocks conflicting with b, then they will send a 0-vote for b back to pᵢ. Note that 0-votes are sent only to the block creator, rather than to all processes.
(3) Upon receiving n−f 0-votes for b, pᵢ will then form a 0-QC for b and send this to all processes.

When a block b′ wishes to point to b, it will include a z-QC for b (for some z ∈ {0, 1, 2}). As a consequence, any process will be able to check that b′ is valid/properly formed without actually receiving the blocks that b′ points to: the existence of QCs for those blocks suffices to ensure that they are properly formed (and that at least one correct process has those blocks), and other requirements for the validity of b′ can be checked by direct inspection. For this to work, votes (and QCs) must specify certain properties of the block beyond its hash, such as the height of the block and the block creator. The details are given in Section 5.

## 5 MORPHEUS: THE FORMAL SPECIFICATION

The pseudocode uses a number of local variables, functions, objects and procedures, detailed below. In what follows, we suppose that, when a correct process sends a message to 'all processes', it regards that message as immediately received by itself. All messages are signed by the sender. For any variable x, we write x↓ to denote that x is defined, and x↑ to denote that x is undefined. Table 1 lists all message types.

**Table 1. Message types.**

| Message type | Description |
|--------------|-------------|
| **Blocks** | |
| Genesis block | Unique block of height 0 |
| Transaction blocks | Contain transactions |
| Leader blocks | Used to totally order transaction blocks |
| **Votes and QCs** | |
| 0-votes | Guarantee data availability and non-equivocation in high throughput |
| 1-votes | Sent during 1st round of voting on a block |
| 2-votes | Sent during 2nd round of voting on a block |
| z-QC, z ∈ {0, 1, 2} | formed from n−f z-votes |
| **View messages** | |
| End-view messages | Indicate wish to enter next view |
| (v+1)-certificate | Formed from f+1 end-view v messages |
| View v message | Sent to the leader at start of view v |

**The genesis block.** There exists a unique genesis block, denoted bₐ. For any block b, b.type specifies the type of the block b, b.view is the view corresponding to the block, b.h specifies the height of the block, b.auth is the block creator, and b.slot specifies the slot corresponding to the block. For bₐ, we set:
* bₐ.type = gen, bₐ.view = −1, bₐ.h = 0, bₐ.auth = ⊥, bₐ.slot = 0.

**A comment on the use of slots.** Each block will either be the genesis block, a transaction block, or a leader block. If pᵢ ∈ Π is correct then, for s ∈ ℕ≥₀, pᵢ will produce a single transaction block b with b.slot = s before producing any transaction block b′ with b′.slot = s+1.

Similarly, if pᵢ ∈ Π is correct then, for s ∈ ℕ≥₀, pᵢ will produce a single leader block b with b.slot = s before producing any leader block b′ with b′.slot = s+1.

**z-votes.** For z ∈ {0, 1, 2}, a z-vote for the block b is a message of the form (z, b.type, b.view, b.h, b.auth, b.slot, H(b)), signed by some process in Π. The reason votes include more information than just the hash of the block is explained in Section 4. A z-quorum for b is a set of n−f z-votes for b, each signed by a different process in Π. A z-QC for b is the message m = (z, b.type, b.view, b.h, b.auth, b.slot, H(b)) together with a threshold signature for m, formed from a z-quorum for b using the threshold signature scheme.

**QCs.** By a QC for the block b, we mean a z-QC for b, for some z ∈ {0, 1, 2}. If q is a z-QC for b, then we set q.b = b, q.z = z, q.type = b.type, q.view = b.view, q.h = b.h, q.auth = b.auth, q.slot = b.slot. We define a preordering ≤ on QCs as follows: QCs are preordered first by view, then by type with lead < Tr, and then by height. The variable Mᵢ. Each process pᵢ maintains a local variable Mᵢ, which is automatically updated and specifies the set of all received messages. Initially, Mᵢ contains bₐ and a 1-QC for bₐ.

**Transaction blocks.** Each transaction block b is entirely specified by the following values:
* b.type = Tr, b.view = v ∈ ℕ≥₀, b.h = h ∈ ℕ>₀, b.slot = s ∈ ℕ≥₀.
* b.auth ∈ Π: the block creator.
* b.Tr: a sequence of transactions.
* b.prev: a non-empty set of QCs for blocks of height < h.
* b.1-QC: a 1-QC for a block of height < h.

If b.prev contains a QC for b′, then we say that b points to b′. For b to be valid, we require that it is of the form above and:
(1) b is signed by b.auth.
(2) If s > 0, b points to b′ with b′.type = Tr, b′.auth = b.auth and b′.slot = s − 1.
(3) If b points to b′, then b′.view ≤ b.view.
(4) If h′ = max{b′.h : b points to b′}, then h = h′ + 1.

We suppose correct processes ignore transaction blocks that are not valid. In what follows we therefore adopt the convention that, by a 'transaction block', we mean a 'valid transaction block'.

**A comment on transaction blocks.** During periods of high throughput, a transaction block produced by pᵢ for slot s will just point to pᵢ's transaction block for slot s−1. During periods of low throughput, if there is a unique block b′ received by pᵢ that does not conflict with any other block received by pᵢ, any transaction block b produced by pᵢ will also point to b′ (so that b does not conflict with b′).

The use of b.1-QC is as follows: once correct pᵢ sees a 1-QC q, it will not vote for any transaction block b unless b.1-QC is greater than or equal to q. Ultimately, this will be used to argue that consistency is satisfied.

**When blocks observe each other.** The genesis block observes only itself. Any other block b observes itself and all those blocks observed by blocks that b points to. If two blocks do not observe each other, then they conflict. We write [b] to denote the set of all blocks observed by b.

**The leader of view v.** The leader of view v, denoted lead(v), is process pᵢ, where i = v mod n.

**End-view messages.** If process pᵢ sees insufficient progress during view v, it may send an end-view v message of the form (v), signed by pᵢ. By a quorum of end-view v messages, we mean a set of f+1 end-view v messages, each signed by a different process in Π. If pᵢ receives a quorum of end-view v messages before entering view v+1, it will combine them (using the threshold signature scheme) to form a (v+1)-certificate. Upon first seeing a (v+1)-certificate, pᵢ will send this certificate to all processes and enter view v+1. This ensures that, if some correct process is the first to enter view v+1 after GST, all correct processes enter that view (or a later view) within time Δ.

**View v messages.** When pᵢ enters view v, it will send to lead(v) a view v message of the form (v, q), signed by pᵢ, where q is a maximal amongst 1-QCs seen by pᵢ. We say that q is the 1-QC corresponding to the view v message (v, q).

**A comment on view v messages.** The use of view v messages is to carry out view changes in the same manner as PBFT. When producing the first leader block b of the view, the leader must include a set of n−f view v messages, which act as a justification for the block proposal: the value b.1-QC must be greater than or equal all 1-QCs corresponding to those n−f view v messages. For each subsequent leader block b′ produced in the view, b′.1-QC must be a 1-QC for the previous leader block (i.e., that for the previous slot). The argument for consistency will thus employ some of the same methods as are used to argue consistency for PBFT.

**Leader blocks.** Each leader block b is entirely specified by the following values:
* b.type = lead, b.view = v ∈ ℕ≥₀, b.h = h ∈ ℕ>₀, b.slot = s ∈ ℕ≥₀.
* b.auth ∈ Π: the block creator.
* b.prev: a non-empty set of QCs for blocks of height < h.
* b.1-QC: a 1-QC for a block of height < h.
* b.just: a (possibly empty) set of view v messages.

As for transaction blocks, if b.prev contains a QC for b′, then we say that b points to b′. For b to be valid, we require that it is of the form described above and:
(1) b is signed by b.auth and b.auth = lead(v).
(2) If b points to b′, then b′.view ≤ b.view.
(3) If h′ = max{b′.h : b points to b′}, then h = h′ + 1.
(4) If s > 0, b points to a unique b* with b*.type = lead, b*.auth = b.auth and b*.slot = s − 1.
(5) If s = 0 or b*.view < v, then b.just contains n−f view v messages, each signed by a different process in Π. This set of messages is called a justification for the block.
(6) If s = 0 or b*.view < v, then b.1-QC is greater than or equal to all 1-QCs corresponding to view v messages in b.just.
(7) If s > 0 and b*.view = v, then b.1-QC is a 1-QC for b*.

As with transaction blocks, we suppose correct processes ignore leader blocks that are not valid. In what follows we therefore adopt the convention that, by a 'leader block', we mean a 'valid leader block'.

**A comment on leader blocks.** The conditions for validity above are just those required to carry out a PBFT-style approach to view changes (as discussed previously). The first leader block of the view must include a justification for the block proposal (to guarantee consistency). Subsequent leader blocks in the view simply include a 1-QC for the previous leader block (i.e., that for the previous slot).

**The variable Qᵢ.** Each process pᵢ maintains a local variable Qᵢ, which is automatically updated and, for each z ∈ {0, 1, 2}, stores at most one z-QC for each block: For z ∈ {0, 1, 2}, if pᵢ receives a z-quorum or a z-QC for b, and if Qᵢ does not contain a z-QC for b, then pᵢ automatically enumerates a z-QC for b into Qᵢ (either the z-QC received, or one formed from the z-quorum received).

We define the 'observes' relation ⪰ on Qᵢ to be the minimal preordering satisfying (transitivity and):
* If q, q′ ∈ Qᵢ, q.type = q′.type, q.auth = q′.auth and q.slot > q′.slot, then q ⪰ q′.
* If q, q′ ∈ Qᵢ, q.type = q′.type, q.auth = q′.auth, q.slot = q′.slot, and q.z ≥ q′.z, then q ⪰ q′.
* If q, q′ ∈ Qᵢ, q.b = b, q′.b = b′, b ∈ Mᵢ and b points to b′, then q ⪰ q′.

We note that the observes relation ⪰ depends on Qᵢ and Mᵢ, and is stronger than the preordering ≥ we defined on z-QCs previously, in the following sense: if q and q′ are z-QCs with q ⪰ q′, then q ≥ q′, while the converse may not hold. When we refer to the 'greatest' QC in a given set, or a 'maximal' QC in a given set, this is with reference to the ≥ preordering, unless explicitly stated otherwise. If q.type = q′.type, q.auth = q′.auth and q.slot = q′.slot, then it will follow that q.b = q′.b.

**A comment on the observes relation on Qᵢ.** When pᵢ receives q, q′ ∈ Qᵢ, it may not be immediately apparent whether q.b observes q′.b. The observes relation defined on Qᵢ above is essentially that part of the observes relation on blocks that pᵢ can testify to, given the messages it has received (while also distinguishing the 'level' of the QC).

**The tips of Qᵢ.** The tips of Qᵢ are those q ∈ Qᵢ such that there does not exist q′ ∈ Qᵢ with q′ ≻ q (i.e. q′ ⪰ q and q⪰̸ q′). The protocol ensures that Qᵢ never contains more than 2n tips: The factor 2 here comes from the fact that leader blocks produced by correct pᵢ need not observe all transaction blocks produced by pᵢ (and vice versa).

**Single tips.** We say q ∈ Qᵢ is a single tip of Qᵢ if q ⪰ q′ for all q′ ∈ Qᵢ. We say b ∈ Mᵢ is a single tip of Mᵢ if there exists q which is a single tip of Qᵢ and b is the unique block in Mᵢ pointing to q.b.

**A comment on single tips.** When a transaction block is a single tip of Mᵢ, this will enable pᵢ to send a 1-vote for the block. Leader blocks do not have to be single tips for correct processes to vote for them.

**The voted function.** For each i, j, s, z ∈ {0, 1, 2} and x ∈ {lead, Tr}, the value votedᵢ(z, x, s, pⱼ) is initially 0. When pᵢ sends a z-vote for a block b with b.type = x, b.auth = pⱼ, and b.slot = s, it sets votedᵢ(z, x, s, pⱼ) := 1. Once this value is set to 1, pᵢ will not send a z-vote for any block b′ with b′.type = x, b′.auth = pⱼ, and b′.slot = s.

**The phase during the view.** For each i and v, the value phaseᵢ(v) is initially 0. Once pᵢ votes for a transaction block during view v, it will set phaseᵢ(v) := 1, and will then not vote for leader blocks within view v.

**A comment on the phase during a view.** As noted previously, each view can be thought of as consisting of two phases. Initially, the leader is responsible for finalizing transactions. If, after some time, the protocol enters a period of low throughput, then the leader will stop producing leader blocks, and transactions blocks can then be finalized directly. Once a process votes for a transaction block, it may be considered as having entered the low throughput phase of the view. The requirement that it should not then vote for subsequent leader blocks in the view is made so as to ensure consistency between finalized leader blocks and transaction blocks within the view.

**When blocks are final.** Process pᵢ regards q ∈ Qᵢ (and q.b) as final if there exists q′ ∈ Qᵢ such that q′ ⪰ q and q is a 2-QC (for any block).

**The function F.** This is defined exactly as specified in Section 4.

**The variables viewᵢ and slotᵢ(x) for x ∈ {lead, Tr}.** These record the present view and slot numbers for pᵢ.

**The PayloadReadyᵢ function.** We remain agnostic as to how frequently processes should produce transaction blocks, i.e. as to whether processes should produce transaction blocks immediately upon having new transactions to process, or wait until they have a set of new transactions of at least a certain size. We suppose simply that:
* Extraneous to the explicit instructions of the protocol, PayloadReadyᵢ may be set to 1 at some timeslots of the execution.
* If PayloadReadyᵢ = 1 and slotᵢ(Tr) = s > 0, then there exists q ∈ Qᵢ with q.auth = pᵢ, q.type = Tr and q.slot = s − 1.

**A comment on the PayloadReadyᵢ function.** The second requirement above is required so that pᵢ can ensure that the new transaction block it forms can point to its transaction block for the previous slot.

**The procedure MakeTrBlockᵢ.** When pᵢ wishes to form a new transaction block b, it will run this procedure, by executing the following instructions:
(1) Set b.type := Tr, b.auth := pᵢ, b.view := viewᵢ, b.slot := slotᵢ(Tr).
(2) Let s := slotᵢ(Tr). If s > 0, then let q₁ ∈ Qᵢ be such that q₁.auth = pᵢ, q₁.type = Tr and q₁.slot = s − 1. If s = 0, let q₁ be a 1-QC for bₐ. Initially, set b.prev := {q₁}.
(3) If there exists q₂ ∈ Qᵢ which is a single tip of Qᵢ, then enumerate q₂ into b.prev.
(4) If h′ = max{q.h : q ∈ b.prev}, then set b.h := h′ + 1.
(5) Let q be the greatest 1-QC in Qᵢ. Set b.1-QC := q.
(6) Sign b with the values specified above, and send this block to all processes.
(7) Set slotᵢ(Tr) := slotᵢ(Tr) + 1;

**The boolean LeaderReadyᵢ.** At any time, this boolean is equal to 1 iff either of the following conditions are satisfied, setting v = viewᵢ:
(1) Process pᵢ has not yet produced a block b with b.view = v and b.type = lead, and both:
   (a) Process pᵢ has received view v messages signed by at least n−f processes in Π.
   (b) slotᵢ(lead) = 0 or Qᵢ contains q with q.auth = pᵢ, q.type = lead, q.slot = slotᵢ(lead) − 1.
(2) Process pᵢ has previously produced a block b with b.view = v and b.type = lead, and Qᵢ contains a 1-QC for b′ with b′.auth = pᵢ, b′.type = lead, b′.slot = slotᵢ(lead) − 1.

**A comment on the boolean LeaderReadyᵢ.** If pᵢ is the leader for view v, then before producing the first leader block of the view, it must receive view v messages from n−f different processes, and must also receive a QC for the last leader block it produced (if any). Before producing any subsequent leader block in the view, it must receive a 1-QC for the previous leader block.

**The procedure MakeLeaderBlockᵢ.** When pᵢ wishes to form a new leader block b, it will run this procedure, by executing the following instructions:
(1) Set b.type := lead, b.auth := pᵢ, b.view := viewᵢ, b.slot := slotᵢ(lead).
(2) Initially, set b.prev to be the tips of Qᵢ.
(3) Set s := slotᵢ(Tr) and v := viewᵢ. If s > 0, then let q ∈ Qᵢ be such that q.auth = pᵢ, q.type = lead and q.slot = s − 1. If b.prev does not already contain q, add q to this set.
(4) If h′ = max{q.h : q ∈ b.prev}, then set b.h := h′ + 1.
(5) If pᵢ has not yet produced a block b with b.view = viewᵢ and b.type = lead then:
   (a) Set b.just to be a set of view v messages signed by n−f processes in Π.
   (b) Set b.1-QC to be a 1-QC in Qᵢ greater than or equal to all 1-QCs corresponding to messages in b.just.
(6) If pᵢ has previously produced a block b with b.view = viewᵢ and b.type = lead then let q′ ∈ Qᵢ be a 1-QC with q′.auth = pᵢ, q′.type = lead and q′.slot = s − 1. Set b.1-QC := q′ and set b.just to be the empty set.
(7) Sign b with the values specified above, and send this block to all processes.
(8) Set slotᵢ(lead) := slotᵢ(lead) + 1;

**The pseudocode.** The pseudocode appears in Algorithm 1 (with local variables described first, and the main code appearing later). Section 5.1 gives a 'pseudocode walk-through'.

```
Algorithm 1 Morpheus: local variables for pᵢ
1: Local variables
2: Mᵢ, initially contains bₐ and a 1-QC-certificate for bₐ  ⊲ Automatically updated
3: Qᵢ, initially contains 1-QC-certificate for bₐ  ⊲ Automatically updated
4: viewᵢ, initially 0  ⊲ The present view
5: slotᵢ(x) for x ∈ {lead, Tr}, initially 0  ⊲ Present slot
6: votedᵢ(z, x, s, pⱼ) for z ∈ {0, 1, 2}, x ∈ {lead, Tr}, s ∈ ℕ≥₀, pⱼ ∈ Π, initially 0
7: phaseᵢ(v) for v ∈ ℕ≥₀, initially 0  ⊲ The phase within the view
8: Other procedures and functions
9: lead(v)  ⊲ Leader of view v
10: PayloadReadyᵢ  ⊲ Set to 1 when ready to produce transaction block
11: MakeTrBlockᵢ  ⊲ Sends a new transaction block to all
12: LeaderReadyᵢ  ⊲ Indicates whether ready to produce leader block
13: MakeLeaderBlockᵢ  ⊲ Sends a new leader block to all
```

### 5.1 Pseudocode walk-through

**Lines 16-22:** These lines are responsible for view changes. If pᵢ has received a quorum of end-view v messages for some greatest v greater than or equal to its present view, then it will use those to form a (v+1)-certificate and will send that certificate to all processes (immediately regarding that certificate as received and belonging to Mᵢ). Upon seeing that it has received a v-certificate for some greatest view v greater than its present view, pᵢ will: (i) enter view v, (ii) send that v-certificate to all processes, and (iii) send a view v message to the leader of view v, along with any tips of Qᵢ corresponding to its own blocks. Process pᵢ will also do the same upon seeing q with q.view greater than its present view: the latter action ensures that any block b produced by pᵢ during view v does not point to any b′ with b′.view > b.view.

**Lines 24-28.** These lines are responsible for the production of 0-QCs. Upon producing any block, pᵢ sends it to all processes. Providing pᵢ is correct, meaning that the block is correctly formed etc, other processes will then send back a 0-vote for the block to pᵢ, who will form a 0-QC and send it to all processes.

**Lines 30 and 31.** These lines are responsible for producing new transaction blocks. Line 30 checks to see whether pᵢ is ready to produce a new transaction block, before line 31 produces the new block: PayloadReadyᵢ and MakeTrBlockᵢ are specified in Section 5.

```
Algorithm 1 Morpheus: The instructions for pᵢ
14: Process pᵢ executes the following transitions at timeslot t (according to its local clock), until
no further transitions apply. If multiple transitions apply simultaneously, then pᵢ executes the
first that applies, before checking whether further transitions apply, and so on.
15: ⊲ Update view
16: If there exists greatest v ≥ viewᵢ s.t. Mᵢ contains at least f+1 end-view v messages then:
17: Form a (v+1)-certificate and send it to all processes;
18: If there exists some greatest v > viewᵢ such that either:
19: (i) Mᵢ contains a v-certificate q, or (ii) Qᵢ contains q with q.view = v, then:
20: Set viewᵢ := v; Send (either) q to all processes;
21: Send all tips q′ of Qᵢ such that q′.auth = pᵢ to lead(v);
22: Send (v, q′) signed by pᵢ to lead(v), where q′ is a maximal amongst 1-QCs seen by pᵢ
23: ⊲ Send 0-votes and 0-QCs
24: If Mᵢ contains some b s.t. votedᵢ(0, b.type, b.slot, b.auth) = 0:
25: Send a 0-vote for b (signed by pᵢ) to b.auth; Set votedᵢ(0, b.type, b.slot, b.auth) := 1;
26: If Mᵢ contains a 0-quorum for some b s.t.:
27: (i) b.auth = pᵢ, and (ii) pᵢ has not previously sent a 0-QC for b to other processors, then:
28: Send a 0-QC for b to all processes;
29: ⊲ Send out a new transaction block
30: If PayloadReadyᵢ = 1 then:
31: MakeTrBlockᵢ;
32: ⊲ Send out a new leader block
33: If pᵢ = lead(viewᵢ), LeaderReadyᵢ = 1, phaseᵢ(viewᵢ) = 0 and Qᵢ does not have a single tip:
34: MakeLeaderBlockᵢ;
35: ⊲ Send 1 and 2-votes for transaction blocks
36: If there exists b ∈ Mᵢ with b.type = lead and b.view = viewᵢ and
37: there does not exist unfinalized b ∈ Mᵢ with b.type = lead and b.view = viewᵢ then:
38: If there exists b ∈ Mᵢ with b.type = Tr, b.view = viewᵢ and which is a single tip of Mᵢ s.t.:
39: (i) b.1-QC is greater than or equal to every 1-QC in Qᵢ and;
40: (ii) votedᵢ(1, Tr, b.slot, b.auth) = 0, then:
41: Send a 1-vote for b to all processes; Set phaseᵢ(viewᵢ) := 1;
42: Set votedᵢ(1, Tr, b.slot, b.auth) := 1;
43: If there exists a 1-QC q ∈ Qᵢ which is a single tip of Qᵢ s.t.:
44: (i) q.type = Tr and (ii) votedᵢ(2, Tr, q.slot, q.auth) = 0, then:
45: If there does not exist b ∈ Mᵢ of height greater than q.h:
46: Send a 2-vote for q.b to all processes; Set phaseᵢ(viewᵢ) := 1;
47: Set votedᵢ(2, Tr, q.slot, q.auth) := 1;
48: ⊲ Vote for a leader block
49: If phase(viewᵢ) = 0:
50: If ∃b ∈ Mᵢ with b.type = lead, b.view = viewᵢ, votedᵢ(1, lead, b.slot, b.auth) = 0 then:
51: Send a 1-vote for b to all processes; Set votedᵢ(1, lead, b.slot, b.auth) := 1;
52: If ∃q ∈ Qᵢ which is a 1-QC with votedᵢ(2, lead, q.slot, q.auth) = 0, q.type = lead,
53: q.view = viewᵢ, then:
54: Send a 2-vote for q.b to all processes; Set votedᵢ(2, lead, q.slot, q.auth) := 1;
55: ⊲ Complain
56: If ∃q ∈ Qᵢ which is maximal according to ⪰ amongst those that have not been finalized for
time 6Δ since entering view viewᵢ:
57: Send q to lead(viewᵢ) if not previously sent;
58: If ∃q ∈ Qᵢ which has not been finalized for time 12Δ since entering view viewᵢ:
59: Send the end-view message (viewᵢ) signed by pᵢ to all processes;
```

**Lines 33 and 34.** These lines are responsible for producing new leader blocks. Line 33 ensures that only the leader is asked to produce leader blocks, that it will only do so once ready (having received QCs for previous leader blocks, as required), and only when required to (only if Qᵢ does not have a single tip and if still in the first phase of the view). LeaderReadyᵢ and MakeLeaderBlockᵢ are specified in Section 5.

**Lines 36-47.** These lines are responsible for determining when correct processes produce 1 and 2-votes for transaction blocks. Lines 36 and 37 dictate that no correct process produces 1 or 2-votes for transaction blocks while in view v until at least one leader block for the view has been finalized (according to the messages they have received), and only if there do not exist unfinalized leader blocks for the view. Given these conditions, pᵢ will produce a 1-vote for any transaction block b that is a single tip of Mᵢ, so long as b.1-QC is greater than or equal to any 1-QC it has seen. It will produce a 2-vote for a transaction block b if there exists q with q.b = b which is a single tip of Qᵢ and if pᵢ has not seen any block of greater height. The latter condition is required to ensure that pᵢ cannot produce a 1-vote for some b′ of greater height than b, and then produce a 2-vote for b (this fact is used in the proof of Theorem 6.2). After producing any 1 or 2-vote for a transaction block while in view v, pᵢ enters the second phase of the view and will no longer produce 1 or 2-votes for leader blocks while in view v.

**Lines 49-54.** These lines are responsible for determining when correct processes produce 1 and 2-votes for leader blocks. Correct processes will only produce such votes while in the first phase of the view.

**Lines 56-59.** These lines are responsible for the production of new-view messages. The proof of Theorem 6.3 justifies the choice of 6Δ and 12Δ.

## 6 ESTABLISHING CONSISTENCY AND LIVENESS

Let M* be the set of all messages received by any process during the execution. Towards establishing consistency, we first prove the following lemma.

**Lemma 6.1.** If q, q′ ∈ M* are 1-QCs with q ≤ q′ and q′ ≤ q, then q.b = q′.b.

**Proof.** Suppose q.view = q′.view, q.type = q′.type, and q.h = q′.h. Consider first the case that q.b and q′.b are both leader blocks for the same view. If q.slot = q′.slot, but q.b ≠ q′.b, then no correct process can produce 1-votes for both blocks. This gives an immediate contradiction, since two subsets of Π of size n−f must have a correct process in the intersection, meaning that 1-QCs cannot be produced for both blocks. So, suppose that q′.slot > q.slot. Since each leader block b with b.slot = s > 0 must point to a leader block b′ with b′.auth = b.auth and b′.slot = s − 1, it follows that q′.h > q.h, which also gives a contradiction.

So, consider next the case that q.b and q′.b are distinct transaction blocks. Since both blocks are of the same height, and since any correct process only votes for a block when it is a sole tip of its local value Mᵢ, no correct process can vote for both blocks. Once again, this gives the required contradiction. □

Note that Lemma 6.1 also suffices to establish a similar result for 2-QCs, since no block can receive a 2-QC without first receiving a 1-QC: No correct process produces a 2-vote for any block without first receiving a 1-QC for the block.

Lemma 6.1 suffices to show that we can think of all 1-QCs q ∈ M* as belonging to a hierarchy, ordered by q.view, then by q.type, and then by q.h, such that if q and q′ belong to the same level of this hierarchy then q.b = q′.b.

**Theorem 6.2.** The Morpheus protocol satisfies consistency.

**Proof.** Given the definition of F from Section 4, let us say b′ → b iff:
* b′ = b, or;
* b′ ≠ bₐ and b″ → b, where q = b′.1-QC and b″ = q.b.

To establish consistency it suffices to show the following:

(†): If b has a 1-QC q₁ ∈ M* and also a 2-QC q₂ ∈ M*, then for any 1-QC q ∈ M* such that q ≥ q₁, q.b → b.

Given (†), suppose M₁ ⊆ M₂ ⊆ M*. For each i ∈ {1, 2}, let M′ᵢ be the largest set of blocks in Mᵢ that is downward closed (in the sense specified in Section 4). Let q′ᵢ be a maximal 2-QC in Mᵢ such that q′ᵢ.b ∈ M′ᵢ, and set b*ᵢ = q′ᵢ.b, or if there is no such 2-QC in Mᵢ, set b*ᵢ = bₐ. Let the sequence bₖ, ..., b₁ = bₐ be such that bₖ = b*₂, and, for each j < k, if q = bⱼ₊₁.1-QC, then q.b = bⱼ. From (†) it follows that b*₁ belongs to the sequence bₖ, ..., b₁, so that F(M₂) ⊇ F(M₁).

We establish (†) by induction on the level of the hierarchy to which q belongs. If q ≤ q₁ (and q₁ ≤ q) then the result follows from Lemma 6.1.

For the induction step, suppose that q > q₁ and suppose first that q.type = lead. Let s = q.slot, v = q.view. By validity of q.b, if s > 0, q.b points to a unique b* with b*.type = lead, b*.auth = q.auth and b*.slot = s − 1. If s = 0 or b*.view < v, then q.just (i.e. (q.b).just) contains n−f view v messages, each signed by a different process in Π. Note that, in this case, any correct process that produces a 2-vote for b must do so before sending a view v message. It follows that, in this case, q.1-QC (i.e. (q.b).1-QC) belongs to a level of the hierarchy strictly below q and greater than or equal to that of q₁. The result therefore follows by the induction hypothesis. If s > 0 and b*.view = v, then q.1-QC is a 1-QC-certificate for b*. Once again, q.1-QC therefore belongs to a level of the hierarchy strictly below q and greater than or equal to that of q₁, so that the result follows by the induction hypothesis.

So, suppose next that q.type = Tr. Note that, in this case, any correct process that produces a 2-vote for b must do so before sending a 1-vote for q.b. If q.view > b.view this follows immediately, because a correct process pᵢ only sends 1 or 2-votes for any block b′ while viewᵢ = b′.view. If q.view = b.view and b.type = lead, this follows because no correct process sends 1 or 2-votes for a leader block after having voted for a transaction block within the same view. If q.view = b.view and b.type = Tr, this follows because any correct process only sends a 2-vote for b so long as there does not exist b′ ∈ Mᵢ of height greater than b. Also, any correct process that produces a 2-vote for b will not vote for q.b unless q.1-QC is greater than or equal to any 1-QC it has received. It follows that q.1-QC belongs to a level of the hierarchy strictly below q and greater than or equal to that of q₁. Once again, the result follows by the induction hypothesis. □

**Theorem 6.3.** The Morpheus protocol satisfies liveness.

**Proof.** Towards a contradiction, suppose that correct pᵢ produces a transaction block b, which never becomes finalized (according to the messages received by pᵢ). Note that all correct processes eventually send 0-votes for b to pᵢ, meaning that pᵢ forms a 0-certificate for b, which is eventually received by all correct processes. Since correct processes send end-view messages if a some QC is not finalized for sufficiently long within any given view (see line 58), correct processes must therefore enter infinitely many views. Let v be a view with correct leader, such that the first correct process pⱼ to enter view v does so at some timeslot after GST, and after pᵢ produces b. Process pⱼ sends a v-certificate to all processes upon entering the view, meaning that all correct processes enter the view within time Δ of pⱼ doing so. Upon entering view v, at time t say, note that pᵢ will send a QC for a transaction block b′ that it has produced to the leader. This block b′ has a slot number greater than or equal to that of b. The leader will produce a leader block observing b′ by time t + 3Δ, which will be finalized (according to the messages received by pᵢ) by time t + 6Δ. □

## 7 LATENCY AND COMPLEXITY ANALYSIS

In this section, we discuss latency and communication complexity for Morpheus. As is standard for protocols that operate in partial synchrony, we focus on values for these metrics after GST. In analysing latency for DAG-based protocols, some papers focus the number of 'layers' of the DAG of blocks required for finalization. This hides information, such as the amount of time required to form each layers. We therefore consider the time to finalization expressed in terms of Δ and δ (recall that δ is the unknown and actual least upper-bound on message delay after GST).

**Worst-case latency.** If pᵢ produces a block before entering a view v which begins after GST, and if the block is not finalized while pᵢ is still in view v, then that view must have a faulty leader (if the leader of the view is correct, then the block will be finalized in time O(δ) of pᵢ entering the view and while pᵢ is still in that view). Any such view is of length O(Δ). Worst-case latency is therefore O(fₐΔ), where fₐ is the actual (unknown) number of faulty processes.

**Worst-case latency during high throughput in a view with correct leader.** If correct pᵢ produces a transaction block b at timeslot t, then pᵢ will produce a 0-QC for b by t + 2δ, which will be received by the leader of the view by t + 3δ. The leader may then produce a new block immediately, but, in the worst case, at most 2δ will then pass before the leader produces the next leader block for the view. That leader block (and b) will then be finalized (according to the messages received by pᵢ) by time t + 8δ.

**Optimisations.** The analysis above considers the worst-case latency during high throughput while in a view with correct leader. As described in the Autobahn paper, however, there are a number of optimisations (with corresponding trade-offs) that can be implemented so as to reduce latency in certain 'good cases'. Note first that we can reduce the bound of t + 8δ to t + 7δ by having processes send 0-votes for b 'to all', and then having each process form their own 0-QC for b. (Whether doing so would reduce real latency depends on subtle issues such as connection speeds, and so on.) Autobahn also specifies (see Section 5.52 of [15]) an optimisation whereby a leader may point to a block b (just with a hash rather than a QC) immediately upon receiving it if they believe the block producer to be trustworthy. Other processes will then only vote for the leader block upon receiving a 0-QC for b. Thus reduces the bound to t + 6δ in the 'good case' that all blocks pointed to do receive 0-QCs. Another standard optimisation [16, 19] (also discussed in the 'Fast Path' paragraph of Section 5.2.1 in [15]) allows a leader block to be finalised after receiving a 1-QC formed from n votes, which reduces the bound to t + 5δ in the case that all processes are acting correctly while in the view. Finally, we assumed in the analysis above that 2δ elapses from the time that the leader receives a new transaction block b until the leader produces a new leader block. In the good case that the leader produces the new leader block immediately, the previous bound of t + 5δ reduces to t + 3δ.

**Worst-case latency during low throughput.** We note that a Byzantine adversary can issue transactions causing high throughput. In analysing latency during low throughput, it therefore really makes sense to consider benign faults (omission/crash failures).

If pᵢ issues a transaction block b at time t, and if no transaction blocks conflict with b, then b will be finalized (according to the messages received by p) by t + 3δ. We note that, in a context where each process produces their own transactions, this actually reduces latency by at least δ when compared to protocols such as PBFT and Tendermint, since there is no need to send transactions to the leader before they can be included in a block.

**Amortised complexity during high throughput.** It is common [17, 20] to show that one can achieve linear amortised communication complexity (i.e. communication complexity per transaction) for DAG-based protocols by using batching, i.e. by requiring that each block contain a large number of transactions, which may be O(n) or greater. The downside of this approach is that, depending on the scenario, waiting for a set of transactions of sufficient size before issuing a block may actually increase latency. The use of erasure codes [1, 21] is also sometimes required, which introduces further subtle trade-offs: these cryptographic methods introduce their own latencies. Like Autobahn, Morpheus achieves linear amortised communication complexity during high throughput, without the need for batching or the use of erasure coding.

For the sake of concreteness, suppose that each correct process produces one transaction block in each interval of length δ, and suppose Δ is O(δ). Suppose correct pᵢ issues a transaction block b at time t₁ while in view v₁, and that the first correct process to enter v₁ does so after GST. Suppose b is ultimately finalized at time t₂ while pᵢ is in view v₂ ≥ v₁, meaning that all views in [v₁, v₂) have Byzantine leaders. Let k = v₂ − v₁ + 1. We consider the total communication cost for correct processes between t₁ and t₂, and amortise by dividing that cost by the total number of new transactions finalized during this interval. For any transaction block b′ issued during this interval, sending b′ to all processes and forming a 0-QC for b′ induces linear cost per transaction. For each view in [v₁, v₂), the communication cost induced (between t₁ and t₂) by the sending of 1 and 2-votes by correct processes is O(n²). So, overall, such messages contribute O(kn²) to the communication cost. The messages sent in lines 56-59 of the pseudocode similarly contribute an overall O(kn²) to the communication cost, as do view certificates, new-view messages and all messages sent in lines 16-22 of the pseudocode. Leader blocks sent by the leader of view v₂ prior to t₂ induce O(n²) communication cost. Since Ω(kn) transactions are finalized in the interval [t₁, t₂], this gives an overall amortised communication cost that is O(n).

**Amortised complexity during low throughput.** In low throughput, sending a block of constant size to all processes gives a communication cost O(n), while the sending of 1 and 2-votes for the block gives cost O(n²) (since votes are sent 'to all'). This gives an amortised cost which is O(n²) per transaction. An O(n) bound can be achieved, either by using batching, or else by having votes be sent only to the block producer, rather than to all processes, and then requiring the block producer to distribute 1 and 2-QCs (increasing latency by 2δ). We do not use the latter option, since doing so seems like a false economy: the leader is anyway the 'bottleneck' in this context, which means that in real terms using this option is only likely to increase latency.

**Latency comparison with Autobahn and Sailfish.** Morpheus is essentially the same as Autobahn during high throughput, and so has all the same advantages as Autobahn (low latency and seamless recovery from periods of asynchrony) in that context, while also being quiescent and giving much lower latency during low throughput. 'Seamless recovery' is a concept discussed in the Autobahn paper, and we do not repeat those discussion here.

We use Sailfish as a representative when making comparisons with DAG-based protocols, since (as far as we are aware) Sailfish has the lowest latency amongst such protocols at the present time, at least in the 'good case' that leaders are honest. In fact, apples-to-apples comparisons with Sailfish are difficult, because there are many cases to consider when analysing Sailfish that do not have analogues in the context of analysing Morpheus. Let us suppose, for example, that 'leaders' are correct. Then, for Sailfish, the time it takes for a block to be finalized will depend on whether it happens to be one of the n−f blocks pointed to by the next leader block. In Sailfish, each block is initially reliably broadcast, but there are also a number of ways in which reliable broadcast can be implemented. In Morpheus, we have required that 0-votes be sent only to the block producer (so as to limit communication complexity), but one could alternatively have 0-votes be sent to all processes, reducing worst-case latency by δ, and similar considerations apply when implementing reliable broadcast. To make a comparison that is as fair as possible, we therefore assume that all-to-all communication is used when carrying out reliable broadcast (as in Sailfish), meaning that it takes time at least 2δ when the broadcaster is correct. Correspondingly, we then make overall comparisons with the total worst-case bound of 7δ for Morpheus given in the paragraph on 'optimisations' above, for the case of a view with correct leader (we do not consider the further optimisations for Morpheus that it was previously noted can bring the latency down to 3δ in the 'good case'). We are also generous to Sailfish in considering the latency for blocks that are amongst the n−f blocks pointed to by the leader of the next round. With these assumptions, the latency analyses are then essentially identical for the two protocols. For Sailfish, reliably broadcasting a block takes time 2δ. Upon receiving that block, the next leader may take time up to 2δ to produce their next block, which will then be finalized within a further time 3δ. This gives the same worst-case latency bound of 7δ when leaders are correct. Advantages of Morpheus over Sailfish include the fact that it has linear amortised communication complexity without batching, and much lower latency of 3δ during low throughput.

## 8 RELATED WORK

Morpheus uses a PBFT [10] style approach to view changes, while consistency between finalised transaction blocks within the same view uses an approach similar to Tendermint [8, 9] and Hotstuff [29]. As noted in Section 7, Hotstuff's approach of relaying all messages via the leader could be used by Morpheus during low throughput to decrease communication complexity, but this is unlikely to lead to a decrease in 'real' latency (i.e. actual finalisation times). As also noted in Section 7, the optimistic 'fast commit' of Zyzzyva [16, 19] can also be applied as a further optimisation.

Morpheus transitions between being a leaderless 'linear' blockchain during low throughput to a leader-based DAG-protocol during high throughput. DAG protocols have been studied for a number of years, Hashgraph [4] being an early example. Hashgraph builds an unstructured DAG and suffers from latency exponential in the number of processes. Spectre was another early DAG protocol, designed for the 'permissionless" setting [24], with proof-of-work as the mechanism for sybil resistance. The protocol implements a 'payment system', but does not totally order transactions. Aleph [14] is more similar to most recent DAG protocols in that it builds a structured DAG in which each process proceeds to the next 'round' after receiving blocks from 2f+1 processes corresponding to the previous round, but still has greater latency than modern DAG protocols.

More recent DAG protocols use a variety of approaches to consensus. Narwhal [13] builds a DAG for the purpose of ensuring data availability, from which (one option is that) a protocol like Hotstuff or PBFT can then be used to efficiently establish a total ordering on transactions. DAG-Rider [17], on the other hand, builds the DAG in such a way that a total ordering can be extracted from the structure of the DAG, with zero further communication cost. The protocol proceeds in 'waves', where each wave consists of four rounds, each round building one 'layer' of the DAG. In each round, each process uses an instance of Reliable Broadcast (RBC) to disseminate their block for the round. Each wave has a leader and an expected six rounds (6 sequential RBCs) are required to finalise the leader's block for the first round of the wave. This finalises all blocks observed by that leader block, but other blocks (such as those in the same round as the leader block) may have signicantly greater latency. Tusk [13] is an implementation based on DAG-Rider.

Given the ability of DAG-Rider to handle significantly higher throughput in many settings, when compared to protocols like PBFT that build a linear blockchain, much subsequent work has taken a similar approach, while looking to improve on latency. While DAG-Rider functions in asynchrony, Bullshark [26] is designed to achieve lower latency in the partially synchronous setting. GradedDAG [12] and LightDAG [11] function in asynchrony, but look to improve latency by replacing RBC [7] with weaker primitives, such as consistent broadcast [27]. This means that those protocols solve Extractable SMR (as defined in Section 3), rather than SMR, and that further communication may be required to ensure full block dissemination in executions with faulty processes. Cordial Miners [18] has versions for both partial synchrony and asynchrony and further decreases latency by using the DAG structure (rather than any primitive such as Consistent or Reliable Broadcast) for equivocation exlcusion. Mysticeti [3] builds on Cordial Miners and establishes a mechanism to accommodate multiple leaders within a single round. Shaol [25] and Shoal++ [2] extend Bullshark by establishing a 'pipelining approach' that implements simultaneous instances of Bullshark with a leader in each round. This reduces latency in the good case because one is required to wait less time before reaching a round in which a leader block is finalised. Both of these papers, however, use a 'reputation' system to select leaders, which comes its own trade-offs. Sailfish [23] similarly describes a mechanism where each round has a leader, but does not make use of a reputation system.

As noted previously, the protocol most similar to Morpheus during high throughput is Autobahn [15]. One of the major distinctions between Autobahn and those previously discussed, is that most blocks are only required to point to a single parent. This significantly decreases communication complexity when the number of processes is large and allows one to achieve linear ammortised communction complexity without the use of erasure coding [1, 21] or batching [20].

## REFERENCES

[1] Nicolas Alhaddad, Sourav Das, Sisi Duan, Ling Ren, Mayank Varia, Zhuolun Xiang, and Haibin Zhang. Balanced byzantine reliable broadcast with near-optimal communication and improved computation. In Proceedings of the 2022 ACM Symposium on Principles of Distributed Computing, pages 399–417, 2022.

[2] Balaji Arun, Zekun Li, Florian Suri-Payer, Sourav Das, and Alexander Spiegelman. Shoal++: High throughput dag bft can be fast! arXiv preprint arXiv:2405.20488, 2024.

[3] Kushal Babel, Andrey Chursin, George Danezis, Anastasios Kichidis, Lefteris Kokoris-Kogias, Arun Koshy, Alberto Sonnino, and Mingwei Tian. Mysticeti: Reaching the limits of latency with uncertified dags. arXiv preprint arXiv:2310.14821, 2023.

[4] Leemon Baird. The swirlds hashgraph consensus algorithm: Fair, fast, byzantine fault tolerance. Swirlds Tech Reports SWIRLDS-TR-2016-01, Tech. Rep, 34:9–11, 2016.

[5] Béla Bollobás. Modern graph theory, volume 184. Springer Science & Business Media, 2013.

[6] Dan Boneh, Ben Lynn, and Hovav Shacham. Short signatures from the weil pairing. In International conference on the theory and application of cryptology and information security, pages 514–532. Springer, 2001.

[7] Gabriel Bracha. Asynchronous byzantine agreement protocols. Information and Computation, 75(2):130–143, 1987.

[8] Ethan Buchman. Tendermint: Byzantine fault tolerance in the age of blockchains. PhD thesis, 2016.

[9] Ethan Buchman, Jae Kwon, and Zarko Milosevic. The latest gossip on bft consensus. arXiv preprint arXiv:1807.04938, 2018.

[10] Miguel Castro, Barbara Liskov, et al. Practical byzantine fault tolerance. In OsDI, volume 99, pages 173–186, 1999.

[11] Xiaohai Dai, Guanxiong Wang, Jiang Xiao, Zhengxuan Guo, Rui Hao, Xia Xie, and Hai Jin. Lightdag: A low-latency dag-based bft consensus through lightweight broadcast. Cryptology ePrint Archive, 2024.

[12] Xiaohai Dai, Zhaonan Zhang, Jiang Xiao, Jingtao Yue, Xia Xie, and Hai Jin. Gradeddag: An asynchronous dag-based bft consensus with lower latency. In 2023 42nd International Symposium on Reliable Distributed Systems (SRDS), pages 107–117. IEEE, 2023.

[13] George Danezis, Lefteris Kokoris-Kogias, Alberto Sonnino, and Alexander Spiegelman. Narwhal and tusk: a dag-based mempool and efficient bft consensus. In Proceedings of the Seventeenth European Conference on Computer Systems, pages 34–50, 2022.

[14] Adam Gągol, Damian Leśniak, Damian Straszak, and Michał Świętek. Aleph: Efficient atomic broadcast in asynchronous networks with byzantine nodes. In Proceedings of the 1st ACM Conference on Advances in Financial Technologies, pages 214–228, 2019.

[15] Neil Giridharan, Florian Suri-Payer, Ittai Abraham, Lorenzo Alvisi, and Natacha Crooks. Autobahn: Seamless high speed bft. In Proceedings of the ACM SIGOPS 30th Symposium on Operating Systems Principles, pages 1–23, 2024.

[16] Guy Golan Gueta, Ittai Abraham, Shelly Grossman, Dahlia Malkhi, Benny Pinkas, Michael Reiter, Dragos-Adrian Seredinschi, Orr Tamir, and Alin Tomescu. Sbft: A scalable and decentralized trust infrastructure. In 2019 49th Annual IEEE/IFIP international conference on dependable systems and networks (DSN), pages 568–580. IEEE, 2019.

[17] Idit Keidar, Eleftherios Kokoris-Kogias, Oded Naor, and Alexander Spiegelman. All you need is dag. In Proceedings of the 2021 ACM Symposium on Principles of Distributed Computing, pages 165–175, 2021.

[18] Idit Keidar, Oded Naor, Ouri Poupko, and Ehud Shapiro. Cordial miners: Fast and efficient consensus for every eventuality. arXiv preprint arXiv:2205.09174, 2022.

[19] Ramakrishna Kotla, Lorenzo Alvisi, Mike Dahlin, Allen Clement, and Edmund Wong. Zyzzyva: speculative byzantine fault tolerance. In Proceedings of twenty-first ACM SIGOPS symposium on Operating systems principles, pages 45–58, 2007.

[20] Andrew Miller, Yu Xia, Kyle Croman, Elaine Shi, and Dawn Song. The honey badger of bft protocols. In Proceedings of the 2016 ACM SIGSAC conference on computer and communications security, pages 31–42, 2016.

[21] Kartik Nayak, Ling Ren, Elaine Shi, Nitin H Vaidya, and Zhuolun Xiang. Improved extension protocols for byzantine broadcast and agreement. arXiv preprint arXiv:2002.11321, 2020.

[22] Victor Shoup. Practical threshold signatures. In International Conference on the Theory and Applications of Cryptographic Techniques, pages 207–220. Springer, 2000.

[23] Nibesh Shrestha, Rohan Shrothrium, Aniket Kate, and Kartik Nayak. Sailfish: Towards improving latency of dag-based bft. Cryptology ePrint Archive, 2024.

[24] Yonatan Sompolinsky, Yoad Lewenberg, and Aviv Zohar. Spectre: A fast and scalable cryptocurrency protocol. Cryptology ePrint Archive, 2016.

[25] Alexander Spiegelman, Balaji Arun, Rati Gelashvili, and Zekun Li. Shoal: Improving dag-bft latency and robustness. arXiv preprint arXiv:2306.03058, 2023.

[26] Alexander Spiegelman, Neil Giridharan, Alberto Sonnino, and Lefteris Kokoris-Kogias. Bullshark: Dag bft protocols made practical. In Proceedings of the 2022 ACM SIGSAC Conference on Computer and Communications Security, pages 2705–2718, 2022.

[27] TK Srikanth and Sam Toueg. Simulating authenticated broadcasts to derive simple fault-tolerant algorithms. Distributed Computing, 2(2):80–94, 1987.

[28] Maofan Yin, Dahlia Malkhi, Michael K Reiter, Guy Golan Gueta, and Ittai Abraham. Hotstuff: Bft consensus in the lens of blockchain. arXiv preprint arXiv:1803.05069, 2018.

[29] Maofan Yin, Dahlia Malkhi, Michael K Reiter, Guy Golan Gueta, and Ittai Abraham. Hotstuff: Bft consensus with linearity and responsiveness. In Proceedings of the 2019 ACM Symposium on Principles of Distributed Computing, pages 347–356, 2019.