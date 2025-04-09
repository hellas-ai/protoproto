use crate::components::counter_btn::Button;
use crate::components::process_viewer::{ProcessViewer, ProcessViewerStyles};
use crate::pages::simulation_builder::SimulationBuilder;

use hellas_morpheus::test_harness::MockHarness;
use hellas_morpheus::{Identity, MorpheusProcess};
use leptos::wasm_bindgen::JsCast;
use leptos::{prelude::*, web_sys};

/// Default Home Page
#[component]
pub fn Home() -> impl IntoView {
    view! {
        <ErrorBoundary fallback=|errors| {
            view! {
                <h1>"Uh oh! Something went wrong!"</h1>

                <p>"Errors: "</p>
                // Render a list of errors as strings - good for development purposes
                <ul>
                    {move || {
                        errors
                            .get()
                            .into_iter()
                            .map(|(_, e)| view! { <li>{e.to_string()}</li> })
                            .collect_view()
                    }}

                </ul>
            }
        }>

            <div class="container">
                <h1>"Welcome to Morpheus"</h1>

                <SimulationBuilder />
            </div>
        </ErrorBoundary>
    }
}
