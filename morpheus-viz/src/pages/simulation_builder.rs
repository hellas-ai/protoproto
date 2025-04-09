use hellas_morpheus::test_harness::{MockHarness, TxGenPolicy};
use hellas_morpheus::{Identity, MorpheusProcess};
use leptos::html::Input;
use leptos::prelude::*;
use std::sync::{Arc, RwLock};

use crate::components::process_viewer::{ProcessViewer, ProcessViewerStyles};

#[component]
pub fn SimulationBuilder() -> impl IntoView {
    // Create signals for form inputs
    let (time_step, set_time_step) = signal(1u32);
    let (num_nodes, set_num_nodes) = signal(3u32);
    let (num_byzantine, set_num_byzantine) = signal(1u32);

    // Create signals for validation errors
    let (time_step_error, set_time_step_error) = signal::<Option<String>>(None);
    let (num_nodes_error, set_num_nodes_error) = signal::<Option<String>>(None);
    let (num_byzantine_error, set_num_byzantine_error) = signal::<Option<String>>(None);

    // Create a signal for the process viewer component
    let (harness, set_harness) = signal::<Option<MockHarness>>(None);

    // Reset button text
    let (button_text, set_button_text) = signal("Start new simulation".to_string());

    // Validate time step input
    let validate_time_step = move |value: u32| {
        if value < 1 {
            set_time_step_error(Some("Time step must be at least 1".into()));
            false
        } else {
            set_time_step_error(None);
            true
        }
    };

    // Validate number of nodes
    let validate_num_nodes = move |value: u32| {
        if value < 3 {
            set_num_nodes_error(Some("Number of nodes must be at least 3".into()));
            false
        } else {
            set_num_nodes_error(None);
            true
        }
    };

    // Validate number of byzantine nodes
    let validate_num_byzantine = move |value: u32, nodes: u32| {
        if value >= nodes / 3 {
            set_num_byzantine_error(Some(format!(
                "Byzantine nodes must be less than 1/3 of total nodes (max {})",
                nodes / 3 - 1
            )));
            false
        } else {
            set_num_byzantine_error(None);
            true
        }
    };

    // Handle form submission
    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();

        // Get current values
        let time_step_val = time_step();
        let num_nodes_val = num_nodes();
        let num_byzantine_val = num_byzantine();

        // Validate all inputs
        let is_time_step_valid = validate_time_step(time_step_val);
        let is_num_nodes_valid = validate_num_nodes(num_nodes_val);
        let is_num_byzantine_valid = validate_num_byzantine(num_byzantine_val, num_nodes_val);

        // If all inputs are valid, create the simulation
        if is_time_step_valid && is_num_nodes_valid && is_num_byzantine_valid {
            // Create the nodes
            let nodes = (0..num_nodes_val)
                .map(|i| {
                    MorpheusProcess::new(
                        Identity(i.into()),
                        num_nodes_val as usize,
                        num_byzantine_val as usize,
                    )
                })
                .collect::<Vec<_>>();

            // Create the harness
            let mut new_harness = MockHarness::new(nodes, time_step_val.into());
            
            // Set up default tx generation policies (Never for all nodes)
            for i in 0..num_nodes_val {
                new_harness.tx_gen_policy.insert(Identity(i.into()), TxGenPolicy::Never);
            }

            // Update the harness signal
            set_harness.set(Some(new_harness));

            // Update button text
            set_button_text("Restart simulation".to_string());
        }
    };

    // Handle input changes
    let on_time_step_input = move |ev| {
        let value = event_target_value(&ev).parse::<u32>().unwrap_or_default();
        set_time_step(value);
        validate_time_step(value);
    };

    let on_num_nodes_input = move |ev| {
        let value = event_target_value(&ev).parse::<u32>().unwrap_or_default();
        set_num_nodes(value);
        validate_num_nodes(value);
        // Re-validate byzantine nodes when total nodes change
        validate_num_byzantine(num_byzantine(), value);
    };

    let on_num_byzantine_input = move |ev| {
        let value = event_target_value(&ev).parse::<u32>().unwrap_or_default();
        set_num_byzantine(value);
        validate_num_byzantine(value, num_nodes());
    };
    
    // Simulation control functions
    let run_step = move |_| {
        if let Some(mut h) = harness.get() {
            h.step();
            set_harness.set(Some(h));
        }
    };
    
    let run_multiple_steps = move |_| {
        if let Some(mut h) = harness.get() {
            h.run(5); // Run 5 steps at once
            set_harness.set(Some(h));
        }
    };
    
    // Function to update a node's tx gen policy
    let update_tx_policy = move |node_id: u64, policy_type: &str| {
        if let Some(mut h) = harness.get() {
            let new_policy = match policy_type {
                "never" => TxGenPolicy::Never,
                "always" => TxGenPolicy::Always,
                "every-3" => TxGenPolicy::EveryNSteps { n: 3 },
                "once-per-view" => TxGenPolicy::OncePerView { 
                    prev_view: Arc::new(RwLock::new(None)) 
                },
                _ => TxGenPolicy::Never,
            };
            
            h.tx_gen_policy.insert(Identity(node_id), new_policy);
            set_harness.set(Some(h));
        }
    };

    view! {
        <div class="simulation-builder">
            <ProcessViewerStyles />
            <h2>"Configure Simulation"</h2>

            <form on:submit=on_submit class="simulation-form">
                <div class="form-group">
                    <label for="time-step">"Time Step"</label>
                    <input
                        id="time-step"
                        type="number"
                        value=time_step
                        on:input=on_time_step_input
                        class=move || if time_step_error().is_some() { "input-error" } else { "" }
                    />
                    {move || time_step_error().map(|err| view! { <div class="error-message">{err}</div> })}
                </div>

                <div class="form-group">
                    <label for="num-nodes">"Number of Nodes"</label>
                    <input
                        id="num-nodes"
                        type="number"
                        value=num_nodes
                        on:input=on_num_nodes_input
                        class=move || if num_nodes_error().is_some() { "input-error" } else { "" }
                    />
                    {move || num_nodes_error().map(|err| view! { <div class="error-message">{err}</div> })}
                </div>

                <div class="form-group">
                    <label for="num-byzantine">"Number of Byzantine Nodes"</label>
                    <input
                        id="num-byzantine"
                        type="number"
                        value=num_byzantine
                        on:input=on_num_byzantine_input
                        class=move || if num_byzantine_error().is_some() { "input-error" } else { "" }
                    />
                    {move || num_byzantine_error().map(|err| view! { <div class="error-message">{err}</div> })}
                </div>

                <button type="submit" id="start-simulation">{button_text}</button>
            </form>

            {move || harness.read().clone().map(|h| 
                view! {
                    <div class="simulation-controls">
                        <h3>"Simulation Controls"</h3>
                        <div class="control-buttons">
                            <button on:click=run_step>"Run One Step"</button>
                            <button on:click=run_multiple_steps>"Run 5 Steps"</button>
                        </div>
                        
                        <h3>"TX Generation Policies"</h3>
                        <div class="tx-policies">
                            {h.processes.iter().map(|(id, _)| {
                                let node_id = id.0;
                                let node_id_for_never = node_id;
                                let node_id_for_always = node_id;
                                let node_id_for_every3 = node_id;
                                let node_id_for_once_per_view = node_id;
                                
                                let policy: Option<TxGenPolicy> = h.tx_gen_policy.get(&Identity(node_id)).cloned();
                                let policy2 = policy.clone();
                                let policy3 = policy.clone();
                                let policy4 = policy.clone();
                                view! {
                                    <div class="node-policy">
                                        <div class="node-id">{"Node "} {node_id}</div>
                                        <div class="policy-options">
                                            <button 
                                                on:click=move |_| update_tx_policy(node_id_for_never, "never")
                                                class=move || if matches!(policy, Some(TxGenPolicy::Never) | None) {"active"} else {""}
                                            >"Never"</button>
                                            <button 
                                                on:click=move |_| update_tx_policy(node_id_for_always, "always")
                                                class=move || if matches!(policy2, Some(TxGenPolicy::Always)) {"active"} else {""}
                                            >"Always"</button>
                                            <button 
                                                on:click=move |_| update_tx_policy(node_id_for_every3, "every-3")
                                                class=move || if matches!(policy3, Some(TxGenPolicy::EveryNSteps { n: 3 })) {"active"} else {""}
                                            >"Every 3 Steps"</button>
                                            <button 
                                                on:click=move |_| update_tx_policy(node_id_for_once_per_view, "once-per-view")
                                                class=move || if matches!(policy4, Some(TxGenPolicy::OncePerView { .. })) {"active"} else {""}
                                            >"Once Per View"</button>
                                        </div>
                                    </div>
                                }
                            }).collect_view()}
                        </div>
                    </div>
                }
            )}

            <div class="process-viewer">
                {move || harness.read().clone().map(|h| view! { <ProcessViewer harness=h.into() /> })}
            </div>
            
            <style>
                {r#"
                .simulation-controls {
                    margin-top: 20px;
                    padding: 15px;
                    background-color: #f8f8f8;
                    border-radius: 5px;
                    border: 1px solid #ddd;
                }
                .control-buttons {
                    display: flex;
                    gap: 10px;
                    margin-bottom: 20px;
                }
                .control-buttons button {
                    padding: 8px 15px;
                    background-color: #4a90e2;
                    color: white;
                    border: none;
                    border-radius: 4px;
                    cursor: pointer;
                }
                .control-buttons button:hover {
                    background-color: #357ab8;
                }
                .tx-policies {
                    display: flex;
                    flex-direction: column;
                    gap: 15px;
                }
                .node-policy {
                    display: flex;
                    align-items: center;
                    padding: 10px;
                    background-color: #fff;
                    border-radius: 4px;
                    border: 1px solid #eee;
                }
                .node-id {
                    font-weight: bold;
                    width: 80px;
                }
                .policy-options {
                    display: flex;
                    gap: 5px;
                    flex-wrap: wrap;
                }
                .policy-options button {
                    padding: 5px 10px;
                    background-color: #f0f0f0;
                    border: 1px solid #ddd;
                    border-radius: 3px;
                    cursor: pointer;
                }
                .policy-options button:hover {
                    background-color: #e0e0e0;
                }
                .policy-options button.active {
                    background-color: #4caf50;
                    color: white;
                    border-color: #388e3c;
                }
                .error-message {
                    color: #d32f2f;
                    font-size: 0.85em;
                    margin-top: 5px;
                }
                .input-error {
                    border-color: #d32f2f;
                }
                "#}
            </style>
        </div>
    }
}
