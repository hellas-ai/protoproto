use hellas_morpheus::{
    test_harness::MockHarness, Block, BlockData, BlockKey, BlockType, Identity, Message, Phase, QuorumTrack, Signed, SlotNum, StartView, StateIndex, ThreshSigned, Transaction, ViewNum, VoteData
};
use leptos::prelude::*;
use std::{collections::BTreeMap, sync::Arc};

#[component]
pub fn StartView(start_view: StartView) -> impl IntoView {
    view! {
        <li>
            "StartView(" <ViewNumComponent view=start_view.view /> ", QC: "
            <ThreshSignedComponent qc=Arc::new(start_view.qc.clone()) render_data=|data| view!{ <VoteDataComponent data=data /> }.into_any() />
            ")"
        </li>
    }
}

#[component]
pub fn Block(block: Block) -> impl IntoView {
    view! {
        <div class="block">
            <div class="block-header">
                <span class="block-key">{format!("{:?}", block.key)}</span>
            </div>
            <div class="block-content">
                <div class="block-prev">
                    <span>Previous blocks: {block.prev.len()}</span>
                </div>
                <div class="block-data">
                    <span>Block data type: {match block.data {
                        hellas_morpheus::BlockData::Genesis => "Genesis",
                        hellas_morpheus::BlockData::Tr { .. } => "Transactions",
                        hellas_morpheus::BlockData::Lead { .. } => "Lead",
                    }}</span>
                    {
                        match block.data {
                            hellas_morpheus::BlockData::Tr { transactions } => {
                                view! {
                                    <span>Transactions: {transactions.len()}</span>
                                }.into_any()
                            },
                            hellas_morpheus::BlockData::Lead { justification } => {
                                view! {
                                    <ul>
                                        {
                                            justification.iter().map(|v| view! {
                                                <StartView start_view=v.data.clone() />
                                            }).collect_view()
                                        }
                                    </ul>
                                }.into_any()
                            },
                            _ => view! {}.into_any()
                        }
                    }
                </div>
            </div>
        </div>
    }
}

// NEW: Component for Identity
#[component]
fn IdentityComponent(id: Identity) -> impl IntoView {
    view! { <span>{format!("ID({})", id.0)}</span> }
}

// NEW: Component for ViewNum
#[component]
fn ViewNumComponent(view: ViewNum) -> impl IntoView {
    view! { <span>{format!("V({})", view.0)}</span> }
}

// NEW: Component for SlotNum
#[component]
fn SlotNumComponent(slot: SlotNum) -> impl IntoView {
    view! { <span>{format!("S({})", slot.0)}</span> }
}

// NEW: Component for BlockType
#[component]
fn BlockTypeComponent(block_type: BlockType) -> impl IntoView {
    view! {
        <span>{
            match block_type {
                BlockType::Lead => "Lead".to_string(),
                BlockType::Tr => "Tr".to_string(),
                BlockType::Genesis => "Genesis".to_string(),
            }
        }</span>
    }
}

// NEW: Component for BlockKey
#[component]
fn BlockKeyComponent(key: BlockKey) -> impl IntoView {
    view! {
        <span>
            {format!(
                "{}:{}:{}",
                 key.author.map(|id| id.0.to_string()).unwrap_or("?".to_string()),
                 match key.type_ { BlockType::Lead => "Ld", BlockType::Tr => "Tr", BlockType::Genesis => "Gen" },
                 key.slot.0
            )}
        </span>
    }
}

// NEW: Component for Phase
#[component]
fn PhaseComponent(phase: Phase) -> impl IntoView {
    view! {
        <span>{
            match phase {
                Phase::High => "High Throughput",
                Phase::Low => "Low Throughput",
            }
        }</span>
    }
}

// NEW: Component for VoteData
#[component]
fn VoteDataComponent(data: VoteData) -> impl IntoView {
    view! {
        <span>
            {format!("Vote(z={}, key=", data.z)}
            <BlockKeyComponent key=data.for_which />
            ")"
        </span>
    }
}

// NEW: Component for ThreshSigned<T>
#[component]
fn ThreshSignedComponent<T: 'static + Clone + std::fmt::Debug>(
    qc: Arc<ThreshSigned<T>>,
    #[prop(optional)] render_data: Option<fn(T) -> AnyView>
) -> impl IntoView {
    let data_view = render_data.map(|f| f(qc.data.clone()));
    view! {
        <div class="signed-data">
            // Placeholder for signature visualization if needed later
            // <span class="signature">Sig: ThreshSig</span>
            { data_view }
        </div>
    }
}

// NEW: Component for Signed<T>
#[component]
fn SignedComponent<T: 'static + Clone>(
    signed_data: Arc<Signed<T>>,
    #[prop(optional)] render_data: Option<fn(T) -> AnyView>
) -> impl IntoView {
    let data_view = render_data.map(|f| f(signed_data.data.clone()));
    view! {
        <div class="signed-data">
            <span class="author">Author: <IdentityComponent id=signed_data.author.clone() /></span>
            // Placeholder for signature visualization if needed later
            // <span class="signature">Sig: Present</span>
            { data_view }
        </div>
    }
}

// NEW: Component for BlockData
#[component]
fn BlockDataComponent(data: BlockData) -> impl IntoView {
    view! {
        <div class="block-data">
            <span>Type: {match data {
                BlockData::Genesis => "Genesis",
                BlockData::Tr { .. } => "Transactions",
                BlockData::Lead { .. } => "Lead",
            }}</span>
            {
                match data {
                    BlockData::Tr { transactions } => {
                        view! {
                            <div class="transactions">
                                <span>Transactions: {transactions.len()}</span>
                                // TODO: Add TransactionComponent and list them if needed
                            </div>
                        }.into_any()
                    },
                    BlockData::Lead { justification } => {
                        view! {
                            <div class="justification">
                                <span>Justification ({justification.len()} StartViews):</span>
                                <ul>
                                    {
                                        justification.iter().map(|v| view! {
                                            <SignedComponent signed_data=v.clone() render_data=|sv_data| view! { <StartView start_view=sv_data /> }.into_any() />
                                        }).collect_view()
                                    }
                                </ul>
                            </div>
                        }.into_any()
                    },
                    _ => view! {}.into_any()
                }
            }
        </div>
    }
}

// UPDATED: Block Component
#[component]
pub fn BlockComponent(block: Arc<Signed<Block>>) -> impl IntoView {
    let block_data = block.data.clone(); // Clone inner data for easier access
    view! {
        <div class="block">
            <div class="block-header">
                <span>Block (<BlockKeyComponent key=block_data.key.clone() />) by <IdentityComponent id=block.author.clone() /></span>
            </div>
            <div class="block-content">
                <div class="block-prev">
                    <span>Previous Blocks ({block_data.prev.len()}):</span>
                    <ul class="compact-list">
                    {block_data.prev.iter().map(|prev_key| view! { <li><BlockKeyComponent key=prev_key.data.clone().for_which /></li> }).collect_view()}
                    </ul>
                </div>
                <span>1-QC: <ThreshSignedComponent qc=Arc::new(block_data.one.clone()) render_data=|vd| view!{ <VoteDataComponent data=vd /> }.into_any() /></span>
                <BlockDataComponent data=block_data.data.clone()/>
            </div>
        </div>
    }
}

use serde::{Deserialize, Serialize};

// NEW: Component for QuorumTrack<T>
#[component]
fn QuorumTrackComponent<T: 'static + Ord + Clone + std::fmt::Debug + Serialize + for<'de> Deserialize<'de>>(
    track: QuorumTrack<T>,
    render_key: fn(T) -> AnyView,
    render_value: fn(Arc<Signed<T>>) -> AnyView,
    label: &'static str
) -> impl IntoView {
    view! {
        <details class="quorum-track-details">
            <summary>{label} Votes Tracked: {track.votes.len()}</summary>
            <ul class="compact-list">
            { track.votes.into_iter().map(|(key, value_map)| {
                let key_view = render_key(key);
                let value_views = value_map.into_values().map(|val| render_value(val)).collect_view();
                view! {
                    <li>
                        <div>{key_view} ({value_map.len()} votes):</div>
                        <ul>{value_views}</ul>
                    </li>
                }
            }).collect_view()}
            </ul>
        </details>
    }
}


// NEW: Component for StateIndex
#[component]
fn StateIndexComponent(index: StateIndex) -> impl IntoView {
    view! {
        <div class="state-index-section">
            <h4>State Index</h4>
            <div class="field-row"><span class="field-name">Max View:</span> <span class="field-value"><ViewNumComponent view=index.max_view.0/> (<VoteDataComponent data=index.max_view.1.clone()/>)</span></div>
            <div class="field-row"><span class="field-name">Max Height:</span> <span class="field-value">{index.max_height.0} (<BlockKeyComponent key=index.max_height.1.clone()/>)</span></div>
            <div class="field-row"><span class="field-name">Max 1-QC:</span> <span class="field-value"><ThreshSignedComponent qc=index.max_1qc.clone() render_data=|vd| view!{ <VoteDataComponent data=vd /> }.into_any() /></span></div>

            <details>
                <summary>Blocks ({index.blocks.len()})</summary>
                <ul class="compact-list item-list">
                    {index.blocks.values().map(|b| view! { <li><BlockComponent block=b.clone()/></li> }).collect_view()}
                </ul>
            </details>
            <details>
                <summary>QCs ({index.qcs.len()})</summary>
                <ul class="compact-list item-list">
                   {index.qcs.values().map(|qc| view! { <li><ThreshSignedComponent qc=qc.clone() render_data=|vd| view!{ <VoteDataComponent data=vd /> }.into_any()/></li> }).collect_view()}
                </ul>
            </details>
            <details>
                <summary>All 1-QCs ({index.all_1qc.len()})</summary>
                <ul class="compact-list item-list">
                   {index.all_1qc.iter().map(|qc| view! { <li><ThreshSignedComponent qc=qc.clone() render_data=|vd| view!{ <VoteDataComponent data=vd /> }.into_any() /></li> }).collect_view()}
                </ul>
            </details>
             <details>
                <summary>Tips ({index.tips.len()})</summary>
                <ul class="compact-list item-list">
                    {index.tips.iter().map(|vd| view! { <li><VoteDataComponent data=vd.clone()/></li> }).collect_view()}
                </ul>
            </details>
            <details>
                <summary>Block Pointed By ({index.block_pointed_by.len()})</summary>
                 <ul class="compact-list">
                    {index.block_pointed_by.iter().map(|(key, set)| view! {
                        <li><BlockKeyComponent key=key.clone() /> points to ({set.len()}):
                            <ul class="compact-list"> {set.iter().map(|k| view!{ <li><BlockKeyComponent key=k.clone()/></li>}).collect_view()}</ul>
                        </li>
                    }).collect_view()}
                </ul>
            </details>
             <details>
                <summary>Unfinalized 2-QCs ({index.unfinalized_2qc.len()})</summary>
                <ul class="compact-list item-list">
                    {index.unfinalized_2qc.iter().map(|vd| view! { <li><VoteDataComponent data=vd.clone()/></li> }).collect_view()}
                </ul>
            </details>
             <details>
                <summary>Finalized Status ({index.finalized.len()} entries)</summary>
                 <ul class="compact-list">
                    {index.finalized.iter().map(|(key, status)| view! {
                        <li><BlockKeyComponent key=key.clone() />: {if *status {"Finalized"} else {"Not Finalized"}}</li>
                    }).collect_view()}
                </ul>
            </details>
             <details>
                <summary>Unfinalized QCs by Block ({index.unfinalized.len()})</summary>
                 <ul class="compact-list">
                    {index.unfinalized.iter().map(|(key, set)| view! {
                        <li><BlockKeyComponent key=key.clone() /> has ({set.len()}) unfinalized QCs:
                            <ul class="compact-list item-list"> {set.iter().map(|vd| view!{ <li><VoteDataComponent data=vd.clone()/></li>}).collect_view()}</ul>
                        </li>
                    }).collect_view()}
                </ul>
            </details>
             <details>
                <summary>Contains Lead by View ({index.contains_lead_by_view.len()})</summary>
                 <ul class="compact-list">
                    {index.contains_lead_by_view.iter().map(|(view, status)| view! {
                        <li><ViewNumComponent view=*view />: {if *status {"Seen"} else {"Not Seen"}}</li>
                    }).collect_view()}
                </ul>
            </details>
             <details>
                <summary>Unfinalized Lead by View ({index.unfinalized_lead_by_view.len()})</summary>
                 <ul class="compact-list">
                    {index.unfinalized_lead_by_view.iter().map(|(view, set)| view! {
                        <li><ViewNumComponent view=*view /> has ({set.len()}) unfinalized lead blocks:
                            <ul class="compact-list item-list"> {set.iter().map(|k| view!{ <li><BlockKeyComponent key=k.clone()/></li>}).collect_view()}</ul>
                        </li>
                    }).collect_view()}
                </ul>
            </details>
            // TODO: Add rendering for qc_by_slot, qc_by_view, block_index if needed (they are complex maps)
        </div>
    }
}


// NEW: Component for Message
#[component]
fn MessageComponent(message: Message) -> impl IntoView {
    match message {
        Message::Block(b) => view! { <div>Block: <BlockComponent block=b/></div> }.into_any(),
        Message::NewVote(v) => view! { <div>Vote: <SignedComponent signed_data=v render_data=|vd| view! { <VoteDataComponent data=vd /> }.into_any() /></div> }.into_any(),
        Message::QC(qc) => view! { <div>QC: <ThreshSignedComponent qc=qc render_data=|vd| view! { <VoteDataComponent data=vd /> }.into_any() /></div> }.into_any(),
        Message::EndView(ev) => view! { <div>EndView: <SignedComponent signed_data=ev render_data=|v_num| view! { <ViewNumComponent view=v_num /> }.into_any() /></div> }.into_any(),
        Message::EndViewCert(evc) => view! { <div>EndViewCert: <ThreshSignedComponent qc=evc render_data=|v_num| view! { <ViewNumComponent view=v_num /> }.into_any() /></div> }.into_any(),
        Message::StartView(sv) => view! { <div>StartView: <SignedComponent signed_data=sv render_data=|sv_data| view! { <StartView start_view=sv_data/> }.into_any() /></div> }.into_any(),
    }
}

// TODO: Define PendingVotesComponent if PendingVotes struct is available/needed
// #[component]
// fn PendingVotesComponent(votes: PendingVotes) -> impl IntoView { ... }


// REFACTORED: ProcessViewer Component
#[component]
pub fn ProcessViewer(harness: Signal<MockHarness>) -> impl IntoView {

    let processes = move || {
        // Sort processes by ID for consistent order
        let mut procs: Vec<_> = harness.get().processes.into_iter().collect();
        procs.sort_by_key(|(id, _)| id.0);
        procs
    };

    view! {
        <div class="process-viewer">
            <h1>Morpheus Process State</h1>
            <div class="processes-container">
                {move || processes().into_iter().map(|(id, p)| {
                    let p_clone = p.clone(); // Clone for use inside the view closure
                    view! {
                        <div class="process-card">
                            <h2>Process <IdentityComponent id=id /></h2>

                            <div class="process-section">
                                <h3>Core State</h3>
                                <div class="field-row"><span class="field-name">View:</span> <span class="field-value"><ViewNumComponent view=p_clone.view_i/></span></div>
                                <div class="field-row"><span class="field-name">Lead Slot:</span> <span class="field-value"><SlotNumComponent slot=p_clone.slot_i_lead/></span></div>
                                <div class="field-row"><span class="field-name">Tr Slot:</span> <span class="field-value"><SlotNumComponent slot=p_clone.slot_i_tr/></span></div>
                                <div class="field-row"><span class="field-name">Nodes (n):</span> <span class="field-value">{p_clone.n}</span></div>
                                <div class="field-row"><span class="field-name">Max Faults (f):</span> <span class="field-value">{p_clone.f}</span></div>
                                <div class="field-row"><span class="field-name">Delta:</span> <span class="field-value">{p_clone.delta}</span></div>
                                <div class="field-row"><span class="field-name">Current Time:</span> <span class="field-value">{p_clone.current_time}</span></div>
                                <div class="field-row"><span class="field-name">View Entry Time:</span> <span class="field-value">{p_clone.view_entry_time}</span></div>
                            </div>

                            <div class="process-section">
                                <h3>Phase</h3>
                                <ul class="compact-list">
                                    {p_clone.phase_i.iter().map(|(view, phase)| view! {
                                        <li><ViewNumComponent view=*view />: <PhaseComponent phase=*phase /></li>
                                    }).collect_view()}
                                </ul>
                            </div>

                             <div class="process-section">
                                <h3>Votes Cast (voted_i)</h3>
                                <details>
                                     <summary>Show {p_clone.voted_i.len()} votes</summary>
                                    <ul class="compact-list item-list">
                                        {p_clone.voted_i.iter().map(|(z, block_type, slot, id)| {
                                            view! {
                                                <li>{format!("(z={}, type=", z)} <BlockTypeComponent block_type=*block_type/>, <SlotNumComponent slot=*slot/>, <IdentityComponent id=id.clone()/></li>
                                            }
                                        }).collect_view()}
                                    </ul>
                                </details>
                            </div>

                            <div class="process-section">
                                <h3>Auxiliary Tracking</h3>
                                 <QuorumTrackComponent
                                    track=p_clone.end_views.clone()
                                    render_key=|view_num| view! { <ViewNumComponent view=view_num /> }.into_any()
                                    render_value=|signed_view| view!{ <SignedComponent signed_data=signed_view  /> }.into_any()
                                    label="End View Votes"
                                />
                                <details>
                                     <summary>Zero QCs Sent ({p_clone.zero_qcs_sent.len()})</summary>
                                    <ul class="compact-list item-list">
                                        {p_clone.zero_qcs_sent.iter().map(|k| view! { <li><BlockKeyComponent key=k.clone() /></li> }).collect_view()}
                                    </ul>
                                </details>
                                <details>
                                     <summary>Complained QCs ({p_clone.complained_qcs.len()})</summary>
                                    <ul class="compact-list item-list">
                                        {p_clone.complained_qcs.iter().map(|vd| view! { <li><VoteDataComponent data=vd.clone() /></li> }).collect_view()}
                                    </ul>
                                </details>
                                <QuorumTrackComponent
                                    track=p_clone.vote_tracker.clone()
                                    render_key=|vd| view! { <VoteDataComponent data=vd /> }.into_any()
                                    render_value=|signed_vote| view!{ <SignedComponent signed_data=signed_vote /> }.into_any()
                                    label="Vote Tracker"
                                />
                                <details>
                                    <summary>Start Views Received by View ({p_clone.start_views.len()})</summary>
                                    <ul class="compact-list">
                                        {p_clone.start_views.iter().map(|(view, sv_vec)| view! {
                                            <li><ViewNumComponent view=*view/> ({sv_vec.len()}):
                                                <ul class="compact-list item-list">
                                                   {sv_vec.iter().map(|sv| view! { <li><SignedComponent signed_data=sv.clone() render_data=|sv_data| view!{ <StartView start_view=sv_data/> }.into_any()/></li> }).collect_view()}
                                                </ul>
                                            </li>
                                        }).collect_view()}
                                    </ul>
                                </details>
                                 <details>
                                    <summary>Produced Lead In View ({p_clone.produced_lead_in_view.len()})</summary>
                                     <ul class="compact-list">
                                        {p_clone.produced_lead_in_view.iter().map(|(view, produced)| view! {
                                            <li><ViewNumComponent view=*view />: {if *produced {"Yes"} else {"No"}}</li>
                                        }).collect_view()}
                                    </ul>
                                </details>
                                <div class="field-row"><span class="field-name">Ready Transactions:</span> <span class="field-value">{p_clone.ready_transactions.len()}</span></div>
                                // TODO: Add PendingVotes rendering when component is ready
                                <div class="field-row"><span class="field-name">Pending Votes Map Size:</span> <span class="field-value">{p_clone.pending_votes.len()}</span></div>
                            </div>

                            <div class="process-section">
                                <h3>State Index</h3>
                                <StateIndexComponent index=p_clone.index.clone() />
                            </div>

                             <div class="process-section">
                                <h3>Received Messages</h3>
                                <details>
                                    <summary>{p_clone.received_messages.len()} Total Messages</summary>
                                    // Basic breakdown (could be more detailed)
                                     <div class="message-counts">
                                        <div class="field-row"><span>Blocks:</span> <span>{p_clone.received_messages.iter().filter(|m| matches!(m, Message::Block(_))).count()}</span></div>
                                        <div class="field-row"><span>QCs:</span> <span>{p_clone.received_messages.iter().filter(|m| matches!(m, Message::QC(_))).count()}</span></div>
                                        <div class="field-row"><span>Votes:</span> <span>{p_clone.received_messages.iter().filter(|m| matches!(m, Message::NewVote(_))).count()}</span></div>
                                        <div class="field-row"><span>End Views:</span> <span>{p_clone.received_messages.iter().filter(|m| matches!(m, Message::EndView(_))).count()}</span></div>
                                        <div class="field-row"><span>End View Certs:</span> <span>{p_clone.received_messages.iter().filter(|m| matches!(m, Message::EndViewCert(_))).count()}</span></div>
                                        <div class="field-row"><span>Start Views:</span> <span>{p_clone.received_messages.iter().filter(|m| matches!(m, Message::StartView(_))).count()}</span></div>
                                    </div>
                                    // Full list
                                    <ul class="compact-list item-list message-list">
                                        {p_clone.received_messages.iter().map(|msg| view! { <li><MessageComponent message=msg.clone() /></li> }).collect_view()}
                                    </ul>
                                </details>
                            </div>

                            <div class="process-section">
                                <h3>Genesis</h3>
                                <BlockComponent block=p_clone.genesis.clone() />
                                <div class="field-row"><span class="field-name">Genesis QC:</span> <span class="field-value"><ThreshSignedComponent qc=p_clone.genesis_qc.clone() render_data=|vd| view!{ <VoteDataComponent data=vd /> }.into_any() /></span></div>
                            </div>

                        </div>
                    }
                }).collect_view()}
            </div>
        </div>
    }
}

// Add some CSS for the component styling
#[component]
pub fn ProcessViewerStyles() -> impl IntoView {
    view! {
        <style>
            {r#"
            .process-viewer {
                font-family: system-ui, -apple-system, sans-serif;
                padding: 20px;
                background-color: #f5f5f5; /* Lighter background for better contrast */
                color: #222; /* Darker text for better visibility */
            }

            .processes-container {
                display: grid;
                grid-template-columns: repeat(auto-fill, minmax(450px, 1fr)); /* Slightly wider cards */
                gap: 20px;
            }

            .process-card {
                background-color: white;
                border-radius: 8px;
                box-shadow: 0 3px 6px rgba(0, 0, 0, 0.15); /* Stronger shadow */
                padding: 20px; /* More padding */
                margin-bottom: 20px;
                display: flex;
                flex-direction: column;
                gap: 15px; /* Increased gap between sections */
            }

            .process-section {
                border-bottom: 1px solid #ddd; /* Darker border */
                padding-bottom: 15px; /* More padding */
            }
            .process-section:last-child {
                border-bottom: none;
                padding-bottom: 0;
            }

            .process-section h3, .state-index-section h4 {
                margin-top: 0;
                margin-bottom: 12px;
                font-size: 1.2em; /* Larger headings */
                color: #000; /* Black for better visibility */
            }

            .field-row {
                display: flex;
                justify-content: space-between;
                margin-bottom: 6px; /* More space between rows */
                font-size: 0.95em; /* Larger text */
                align-items: center; /* Vertically align content */
            }

            .field-name {
                font-weight: 600; /* Bolder */
                margin-right: 10px;
                color: #333; /* Darker for better contrast */
            }

            .field-value {
                font-family: monospace;
                text-align: right;
                word-break: break-word; /* Less aggressive than break-all */
                color: #222; /* Darker text */
                background-color: #f8f8f8; /* Light background for values */
                padding: 2px 4px;
                border-radius: 3px;
            }

            details {
                margin-top: 10px;
            }

            details summary {
                cursor: pointer;
                padding: 6px 0;
                font-weight: 600; /* Bolder */
                color: #222; /* Darker for better visibility */
            }
            details summary:hover {
                color: #000;
            }

            .compact-list {
                list-style-type: none;
                padding-left: 15px; /* Indent list */
                margin-top: 8px;
                margin-bottom: 8px;
                font-size: 0.9em; /* Increased from 0.85em */
            }
            .compact-list li {
                margin-bottom: 5px; /* More space between list items */
                color: #222; /* Darker text color */
            }
            .compact-list ul { /* Nested lists */
                padding-left: 15px;
                margin-top: 5px;
            }

            .item-list {
                max-height: 250px; /* Taller lists */
                overflow-y: auto;
                background-color: #f9f9f9;
                padding: 10px;
                border-radius: 4px;
                border: 1px solid #ddd; /* Darker border */
            }
            .message-list li {
                border-bottom: 1px solid #e5e5e5; /* Stronger border */
                padding-bottom: 5px;
                margin-bottom: 5px;
            }
            .message-list li:last-child {
                border-bottom: none;
            }

            .block {
                border: 1px solid #ccc; /* Darker border */
                border-radius: 4px;
                margin: 10px 0;
                padding: 12px;
                background-color: #fdfdfd;
            }

            .block-header {
                font-weight: bold;
                margin-bottom: 8px;
                font-size: 0.95em; /* Larger */
                color: #000; /* Black for better visibility */
            }

            .block-content {
                font-size: 0.9em; /* Increased from 0.85em */
                display: flex;
                flex-direction: column;
                gap: 6px; /* More space */
            }
            .block-content > span { /* Direct span children */
                font-weight: 600; /* Bolder */
                color: #222;
            }

            .block-prev, .block-data, .transactions, .justification {
                margin-left: 12px;
                margin-top: 4px;
            }
            .block-prev > span, .block-data > span, .transactions > span, .justification > span {
                font-weight: 600; /* Bolder */
                color: #222;
            }

            .signed-data {
                display: inline-flex; /* Keep author/sig/data together */
                align-items: baseline;
                gap: 6px;
                font-size: 0.95em; /* Larger */
            }
            .signed-data .author {
                font-style: italic;
                color: #444; /* Darker than original #666 */
            }

            .message-counts {
                background-color: #f0f0f0; /* Darker background */
                padding: 10px;
                border-radius: 4px;
                margin-top: 10px;
                border: 1px solid #ddd; /* Darker border */
            }

            /* Component Specific Styles */
            .IdentityComponent span, .ViewNumComponent span, .SlotNumComponent span, 
            .BlockTypeComponent span, .PhaseComponent span {
                font-family: monospace;
                background-color: #e0e0e0; /* Darker background */
                padding: 2px 5px; /* More padding */
                border-radius: 3px;
                font-size: 0.95em; /* Larger */
                color: #000; /* Black text */
                font-weight: 500; /* Medium weight */
            }
            .BlockKeyComponent span {
                font-family: monospace;
                background-color: #cce5ff; /* Darker blue background */
                padding: 2px 5px; /* More padding */
                border-radius: 3px;
                font-size: 0.95em; /* Larger */
                color: #003366; /* Dark blue text */
                font-weight: 500; /* Medium weight */
            }
            .VoteDataComponent span {
                font-family: monospace;
                background-color: #ddffcc; /* Darker green background */
                padding: 2px 5px; /* More padding */
                border-radius: 3px;
                font-size: 0.95em; /* Larger */
                color: #225500; /* Dark green text */
                font-weight: 500; /* Medium weight */
            }
            "#}
        </style>
    }
}
